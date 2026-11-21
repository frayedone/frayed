//! UI state: one AppData value, 3-tab navigation, direct callbacks into Slint.
//! Freya has no AI tab, no streak badge and no floating callouts — none are wired here.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use frayed_core::storage::{self, AppData};
use frayed_core::{
    context_line, date_label, day_is_cancelled, default_group_preset, effective_blocks, is_newer,
    lesson_progress, lesson_state, minutes_to_time, now_local, spaced, time_to_minutes,
    weather_gradient, weather_label_ru, week_dates, BlockKind, ClassBlock, LocalTime, Schedule,
    WEEKDAY_FULL, WEEKDAY_SHORT,
};
use frayed_net::{
    build_route, stop_arrivals, stop_names, GitHubReleaseClient, OpenMeteoClient, RouteLeg,
    ALMATY_LAT, ALMATY_LON,
};
use frayed_ui::{BlockView, MainWindow};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel, Weak};

pub struct App {
    window: MainWindow,
    data: AppData,
    now: LocalTime,
    selected: usize,
    editing: Option<usize>,
    screen: String,
    transport_mode: i32,
    selected_stop: usize,
    route_from: String,
    route_to: String,
    reminders: HashSet<String>,
}

impl App {
    pub fn new(window: MainWindow) -> Rc<RefCell<Self>> {
        let mut data = storage::load();
        if data.schedule_state.presets.is_empty() {
            data.schedule_state.presets.push(default_group_preset());
            data.schedule_state.active_preset_id = data.schedule_state.presets.first().map(|p| p.id.clone());
            let _ = storage::save(&data);
        }
        let now = now_local();
        let app = Rc::new(RefCell::new(Self {
            window,
            data,
            now,
            selected: now.weekday,
            editing: None,
            screen: "schedule".into(),
            transport_mode: 0,
            selected_stop: 0,
            route_from: "ВУЗ".into(),
            route_to: "КазНУ".into(),
            reminders: HashSet::new(),
        }));
        Self::wire(&app);
        app.borrow().refresh();
        #[cfg(target_os = "android")]
        notify_startup(&app.borrow().data);
        app
    }

    fn preset_name(&self) -> String {
        self.data
            .schedule_state
            .presets
            .iter()
            .find(|p| Some(p.id.as_str()) == self.data.schedule_state.active_preset_id.as_deref())
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "П4Г".into())
    }

    fn schedule(&self) -> Schedule {
        self.data
            .schedule_state
            .presets
            .iter()
            .find(|p| Some(p.id.as_str()) == self.data.schedule_state.active_preset_id.as_deref())
            .map(|p| p.schedule.clone())
            .unwrap_or_default()
    }

    fn blocks(&self) -> Vec<ClassBlock> {
        effective_blocks(&self.schedule(), &self.data.overrides, self.selected)
    }

    /// Pseudo WMO code from the cached flags — enough for the Russian label + hero gradient.
    /// ponytail: store the real Open-Meteo code when the weather cache gains that field.
    fn weather_code(&self) -> i32 {
        match self.data.settings.cached_weather {
            Some(w) if w.is_snow => 71,
            Some(w) if w.is_rain => 61,
            Some(_) => 0,
            None => 0,
        }
    }

    fn refresh(&self) {
        let w = &self.window;
        let now = self.now;
        let dates = week_dates(now.weekday, now.day);
        let blocks = self.blocks();
        let cancelled = day_is_cancelled(&self.schedule(), &self.data.overrides, self.selected);

        w.set_screen(self.screen.clone().into());
        w.set_busy(false);
        w.set_date_label(date_label(now.weekday, now.day, now.month).into());
        w.set_weekday_name(spaced(&WEEKDAY_FULL[self.selected].to_uppercase()).into());
        w.set_is_today(self.selected == now.weekday);
        w.set_group_name(self.preset_name().into());
        w.set_day_labels(strings(WEEKDAY_SHORT.iter().copied()));
        w.set_day_numbers(strings(dates.iter().map(u32::to_string)));

        let day_states = (0..7)
            .map(|d| {
                let mut bits = 0;
                if d == self.selected {
                    bits |= 1;
                }
                if !effective_blocks(&self.schedule(), &self.data.overrides, d).is_empty() {
                    bits |= 2;
                }
                if day_is_cancelled(&self.schedule(), &self.data.overrides, d) {
                    bits |= 4;
                }
                bits
            })
            .collect::<Vec<_>>();
        w.set_day_states(ModelRc::new(VecModel::from(day_states)));
        w.set_selected_day(self.selected as i32);

        w.set_context_line(context_line(&blocks, now.minutes, cancelled).into());
        w.set_day_cancelled(cancelled);
        w.set_empty_day(blocks.is_empty() && !cancelled);
        w.set_blocks(ModelRc::new(VecModel::from(
            blocks
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let key = format!("{}:{}", self.selected, i);
                    block_view(b, now.minutes, cancelled, self.reminders.contains(&key))
                })
                .collect::<Vec<_>>(),
        )));

        let code = self.weather_code();
        let (top, mid, bottom) = weather_gradient(code);
        w.set_hero_top(rgb(top));
        w.set_hero_mid(rgb(mid));
        w.set_hero_bottom(rgb(bottom));
        w.set_weather_condition(weather_label_ru(code).into());

        if let Some(weather) = self.data.settings.cached_weather {
            let temp = format!("{:.0}°", weather.temperature_c);
            let first = blocks.first().map(|b| minutes_to_time(b.start_min));
            let mut hint = format!(
                "На улице {:.0}°, {}",
                weather.temperature_c,
                weather_label_ru(code)
            );
            if let Some(first) = first {
                hint.push_str(&format!(" · к первой паре {first}"));
            }
            if weather.is_rain || weather.is_snow {
                hint.push_str(" · возьми зонт");
            }
            w.set_weather_hint(hint.into());
            w.set_weather_rainy(weather.is_rain || weather.is_snow);
            w.set_weather_temp(temp.clone().into());
            w.set_weather_stats(format!("Ощущается {:.0}°", weather.apparent_temperature_c).into());
            w.set_forecast(strings(vec![format!(
                "Сегодня · {} · {temp}",
                weather_label_ru(code)
            )]));
        } else {
            w.set_weather_hint("Погода ещё не загружена — обнови в меню".into());
            w.set_weather_rainy(false);
            w.set_weather_temp("--°".into());
            w.set_weather_stats("Нет данных".into());
            w.set_forecast(strings(vec!["Нет прогноза".to_string()]));
        }

        let names = stop_names();
        w.set_stop_names(strings(names.iter().copied()));
        w.set_selected_stop(self.selected_stop as i32);
        w.set_transport_mode(self.transport_mode);
        w.set_route_from(self.route_from.clone().into());
        w.set_route_to(self.route_to.clone().into());

        let arrivals: Vec<String> = stop_arrivals(self.selected_stop)
            .iter()
            .map(|a| {
                let eta = if a.eta_min <= 5 {
                    format!("через {} мин", a.eta_min)
                } else {
                    format!("≈ {} мин", a.eta_min)
                };
                format!("{} · {} — {}\n{}", a.route, a.vehicle, eta, a.badge)
            })
            .collect();
        w.set_arrivals(strings(arrivals));

        let legs: Vec<RouteLeg> = build_route(&self.route_from, &self.route_to);
        w.set_itinerary(strings(legs.iter().map(|leg| {
            format!(
                "{} {} · {} → {}\n≈ {} мин",
                leg.vehicle, leg.route, leg.from, leg.to, leg.minutes
            )
        })));

        w.set_settings_text(
            format!(
                "Группа: {}\nУведомления: {}\nПогода: {}\nВерсия: {}\n\nПресеты: {}\n\nТема: тёмная · акцент: янтарь",
                self.preset_name(),
                if self.data.settings.notifications_enabled { "вкл" } else { "выкл" },
                self.data
                    .settings
                    .cached_weather
                    .map(|w| format!("{:.0}°", w.temperature_c))
                    .unwrap_or_else(|| "нет данных".into()),
                env!("CARGO_PKG_VERSION"),
                self.data.schedule_state.presets.len(),
            )
            .into(),
        );
    }

    fn wire(app: &Rc<RefCell<Self>>) {
        let w = app.borrow().window.clone_strong();

        let a = app.clone();
        w.on_day_selected(move |day| {
            let mut s = a.borrow_mut();
            s.selected = (day as usize).min(6);
            s.refresh();
        });

        let a = app.clone();
        w.on_today_pressed(move || {
            let mut s = a.borrow_mut();
            s.now = now_local();
            s.selected = s.now.weekday;
            s.refresh();
        });

        let a = app.clone();
        w.on_nav_pressed(move |screen| {
            let mut s = a.borrow_mut();
            s.screen = screen.to_string();
            s.refresh();
        });

        let a = app.clone();
        w.on_add_pressed(move || {
            let mut s = a.borrow_mut();
            s.editing = None;
            s.screen = "edit".into();
            s.window.set_edit_title("".into());
            s.window.set_edit_room("".into());
            s.window.set_edit_teacher("".into());
            s.window.set_edit_start("09:00".into());
            s.window.set_edit_end("10:30".into());
            s.window.set_edit_kind(0);
            s.refresh();
        });

        let a = app.clone();
        w.on_block_selected(move |index| {
            let mut s = a.borrow_mut();
            let index = index as usize;
            let block = s.blocks().get(index).cloned();
            s.editing = Some(index);
            if let Some(b) = block {
                s.window.set_edit_title(b.title.into());
                s.window.set_edit_room(b.room.into());
                s.window.set_edit_teacher(b.teacher.into());
                s.window.set_edit_start(minutes_to_time(b.start_min).into());
                s.window.set_edit_end(minutes_to_time(b.end_min).into());
                s.window.set_edit_kind(kind_index(b.kind));
            }
            s.screen = "edit".into();
            s.refresh();
        });

        let a = app.clone();
        w.on_edit_close(move || {
            let mut s = a.borrow_mut();
            s.editing = None;
            s.screen = "schedule".into();
            s.refresh();
        });

        let a = app.clone();
        w.on_edit_save(move |title, room, teacher, start, end, kind| {
            let mut s = a.borrow_mut();
            let (Some(start_min), Some(end_min)) =
                (time_to_minutes(&start), time_to_minutes(&end))
            else {
                toast(&s.window, "Неверный формат времени: HH:MM");
                return;
            };
            if end_min <= start_min {
                toast(&s.window, "Конец должен быть позже начала");
                return;
            }
            let block = ClassBlock {
                title: title.trim().to_string(),
                room: room.trim().to_string(),
                teacher: teacher.trim().to_string(),
                start_min,
                end_min,
                kind: kind_from_index(kind),
            };
            let weekday = s.selected;
            let mut blocks = s.blocks();
            match s.editing.take() {
                Some(i) if i < blocks.len() => blocks[i] = block,
                _ => blocks.push(block),
            }
            blocks.sort_by_key(|b| b.start_min);
            apply_blocks(&mut s, weekday, blocks);
            s.screen = "schedule".into();
            s.refresh();
            toast(&s.window, "Расписание сохранено");
        });

        let a = app.clone();
        w.on_remind_pressed(move |index| {
            let mut s = a.borrow_mut();
            let key = format!("{}:{}", s.selected, index);
            if !s.reminders.remove(&key) {
                s.reminders.insert(key);
                toast(&s.window, "Напоминание включено");
            } else {
                toast(&s.window, "Напоминание выключено");
            }
            s.refresh();
        });

        let a = app.clone();
        w.on_cancel_day_pressed(move || {
            let mut s = a.borrow_mut();
            let weekday = s.selected;
            if let Some(o) = s.data.overrides.days.iter_mut().find(|o| o.weekday == weekday) {
                o.cancelled = !o.cancelled;
            } else {
                s.data.overrides.days.push(frayed_core::DayOverride {
                    weekday,
                    cancelled: true,
                    blocks: vec![],
                });
            }
            let _ = storage::save(&s.data);
            s.refresh();
            toast(&s.window, if s.data.overrides.days.iter().any(|o| o.weekday == weekday && o.cancelled) { "День отменён" } else { "День возвращён" });
        });

        let a = app.clone();
        w.on_preset_pressed(move |index| {
            let mut s = a.borrow_mut();
            let chosen = s
                .data
                .schedule_state
                .presets
                .get(index as usize)
                .map(|p| (p.id.clone(), p.name.clone()));
            if let Some((id, name)) = chosen {
                s.data.schedule_state.active_preset_id = Some(id);
                let _ = storage::save(&s.data);
                s.refresh();
                toast(&s.window, format!("Группа: {name}"));
            }
        });

        let a = app.clone();
        w.on_transport_mode_changed(move |mode| {
            let mut s = a.borrow_mut();
            s.transport_mode = mode;
            s.refresh();
        });

        let a = app.clone();
        w.on_stop_selected(move |index| {
            let mut s = a.borrow_mut();
            s.selected_stop = (index as usize).min(stop_names().len().saturating_sub(1));
            s.refresh();
        });

        let a = app.clone();
        w.on_route_built(move |from, to| {
            let mut s = a.borrow_mut();
            s.route_from = from.to_string();
            s.route_to = to.to_string();
            s.refresh();
            toast(&s.window, "Маршрут построен");
        });

        let a = app.clone();
        w.on_refresh_pressed(move || {
            let s = a.borrow();
            s.window.set_busy(true);
            let weak = s.window.as_weak();
            let data = s.data.clone();
            drop(s);
            refresh_network(weak, data);
        });

        let a = app.clone();
        w.on_update_pressed(move || {
            let s = a.borrow();
            s.window.set_busy(true);
            let weak = s.window.as_weak();
            drop(s);
            update_network(weak);
        });

        let a = app.clone();
        w.on_data_reloaded(move || {
            let mut s = a.borrow_mut();
            s.data = storage::load();
            s.refresh();
        });

        let a = app.clone();
        w.on_toast_dismissed(move || {
            a.borrow().window.set_toast("".into());
        });
    }
}

fn apply_blocks(app: &mut App, weekday: usize, blocks: Vec<ClassBlock>) {
    if let Some(o) = app.data.overrides.days.iter_mut().find(|o| o.weekday == weekday) {
        o.cancelled = false;
        o.blocks = blocks;
    } else {
        app.data.overrides.days.push(frayed_core::DayOverride {
            weekday,
            cancelled: false,
            blocks,
        });
    }
    let _ = storage::save(&app.data);
}

fn kind_index(kind: BlockKind) -> i32 {
    match kind {
        BlockKind::Lecture => 0,
        BlockKind::Practice => 1,
        BlockKind::Lab => 2,
        BlockKind::Test => 3,
        _ => 4,
    }
}

fn kind_from_index(index: i32) -> BlockKind {
    match index {
        1 => BlockKind::Practice,
        2 => BlockKind::Lab,
        3 => BlockKind::Test,
        4 => BlockKind::Custom,
        _ => BlockKind::Lecture,
    }
}

fn strings<I: IntoIterator<Item = S>, S: Into<SharedString>>(items: I) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
}

fn rgb(packed: u32) -> Color {
    Color::from_argb_u8(
        255,
        ((packed >> 16) & 0xff) as u8,
        ((packed >> 8) & 0xff) as u8,
        (packed & 0xff) as u8,
    )
}

fn block_view(b: &ClassBlock, now: u32, cancelled: bool, reminded: bool) -> BlockView {
    let kind = match b.kind {
        BlockKind::Lecture => "",
        BlockKind::Lab => "Лаборатория",
        BlockKind::Test => "Тест",
        BlockKind::Practice => "Практика",
        BlockKind::Break => "Перерыв",
        BlockKind::Custom => "",
    };
    let mut meta = Vec::new();
    if !kind.is_empty() {
        meta.push(kind);
    }
    if !b.teacher.is_empty() {
        meta.push(&b.teacher);
    }
    if !b.room.is_empty() {
        meta.push(&b.room);
    }
    let state = lesson_state(b.start_min, b.end_min, now, cancelled);
    BlockView {
        time: minutes_to_time(b.start_min).into(),
        end_time: minutes_to_time(b.end_min).into(),
        title: b.title.clone().into(),
        meta: meta.join(" · ").into(),
        state,
        progress: lesson_progress(b.start_min, b.end_min, now),
        reminded,
    }
}

fn toast(window: &MainWindow, message: impl Into<SharedString>) {
    let message = message.into();
    window.set_toast(message.clone());
    let weak = window.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(4));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                if w.get_toast() == message {
                    w.set_toast("".into());
                }
            }
        });
    });
}

fn spawn<F: std::future::Future<Output = ()> + Send + 'static>(future: F) {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        runtime.block_on(future);
    });
}

fn refresh_network(weak: Weak<MainWindow>, mut data: AppData) {
    spawn(async move {
        if let Ok(weather) = OpenMeteoClient::new()
            .fetch_weather(ALMATY_LAT, ALMATY_LON)
            .await
        {
            data.settings.cached_weather = Some(weather);
            data.settings.last_weather_fetch = now_local().unix_secs;
            let _ = storage::save(&data);
        }
        let (notice, banner) = match GitHubReleaseClient::new().latest_release().await {
            Ok(release) if is_newer(env!("CARGO_PKG_VERSION"), &release.version) => (
                format!("Доступно обновление {}", release.tag_name),
                release.tag_name.clone(),
            ),
            Ok(release) => (format!("Актуальная версия {}", release.tag_name), String::new()),
            Err(_) => ("Нет связи с GitHub".to_string(), String::new()),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                let _ = w.invoke_data_reloaded();
                w.set_update_version(banner.into());
                w.set_toast(notice.into());
                w.set_busy(false);
            }
        });
    });
}

fn update_network(weak: Weak<MainWindow>) {
    spawn(async move {
        let client = GitHubReleaseClient::new();
        let result = match client.latest_release().await {
            Ok(release) if is_newer(env!("CARGO_PKG_VERSION"), &release.version) => {
                let dir = storage::data_path()
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(std::env::temp_dir);
                let path = dir.join("frayed-update.apk");
                match client
                    .download_release_apk(&release.apk_url, &path.to_string_lossy())
                    .await
                {
                    Ok(()) => {
                        #[cfg(target_os = "android")]
                        crate::android::install_apk(&path.to_string_lossy());
                        format!("Установка {}", release.tag_name)
                    }
                    Err(e) => format!("Ошибка загрузки: {e}"),
                }
            }
            Ok(release) => format!("Уже установлена {}", release.tag_name),
            Err(e) => format!("Ошибка проверки: {e}"),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                w.set_toast(result.into());
                w.set_busy(false);
            }
        });
    });
}

#[cfg(target_os = "android")]
fn notify_startup(data: &AppData) {
    let now = now_local();
    let schedule = data
        .schedule_state
        .presets
        .iter()
        .find(|p| Some(p.id.as_str()) == data.schedule_state.active_preset_id.as_deref())
        .map(|p| p.schedule.clone())
        .unwrap_or_default();
    let blocks = effective_blocks(&schedule, &data.overrides, now.weekday);
    if now.minutes < 8 * 60 {
        crate::android::notify("Доброе утро", &format!("Сегодня {} пар", blocks.len()));
    }
}
