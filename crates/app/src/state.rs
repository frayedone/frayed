//! UI state: one AppData value, one selected day, direct callbacks into the Slint window.

use std::cell::RefCell;
use std::rc::Rc;

use frayed_core::date::{month_title_ru, next_class_start, now_local, week_dates, LocalTime};
use frayed_core::storage::{self, AppData};
use frayed_core::{effective_blocks, is_newer, minutes_to_time, BlockKind, ClassBlock, Schedule};
use frayed_net::{fetch_departures, GitHubReleaseClient, OpenMeteoClient};
use frayed_ui::{BlockView, MainWindow};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};

const WEEKDAY_LETTERS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

pub struct App {
    window: MainWindow,
    data: AppData,
    now: LocalTime,
    selected: usize,
    editing: Option<usize>,
}

impl App {
    pub fn new(window: MainWindow) -> Rc<RefCell<Self>> {
        let mut data = storage::load();
        if data.schedule_state.presets.is_empty() {
            data.schedule_state.presets.push(seed_preset());
            data.schedule_state.active_preset_id = Some("main".into());
            let _ = storage::save(&data);
        }
        let now = now_local();
        let app = Rc::new(RefCell::new(Self {
            window,
            data,
            now,
            selected: now.weekday,
            editing: None,
        }));
        Self::wire(&app);
        app.borrow().refresh();
        #[cfg(target_os = "android")]
        notify_startup(&app.borrow().data);
        app
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

    fn refresh(&self) {
        let w = &self.window;
        let now = self.now;
        let dates = week_dates(now.weekday, now.day);

        w.set_status_time(format!("{:02}:{:02}", now.minutes / 60, now.minutes % 60).into());
        w.set_month_title(month_title_ru(now.month, now.year).into());
        w.set_weekday_letters(strings(WEEKDAY_LETTERS.iter().copied()));
        w.set_weekday_numbers(strings(dates.iter().map(u32::to_string)));
        w.set_selected_day(self.selected as i32);

        let blocks = self.blocks();
        w.set_blocks(ModelRc::new(VecModel::from(
            blocks.iter().map(block_view).collect::<Vec<_>>(),
        )));

        let cancelled = self
            .data
            .overrides
            .days
            .iter()
            .any(|o| o.weekday == self.selected && o.cancelled);
        let test_today = blocks.iter().find(|b| b.kind == BlockKind::Test);
        if cancelled {
            w.set_alert_title("День отменён".into());
            w.set_alert_subtitle("Занятий нет — отключите отмену в меню".into());
        } else if let Some(test) = test_today {
            w.set_alert_title("Тест сегодня".into());
            w.set_alert_subtitle(format!("{} · {}", test.title, test.room).into());
        } else if let Some(start) = next_class_start(&blocks, now.minutes) {
            w.set_alert_title("Ближайшая пара".into());
            w.set_alert_subtitle(format!("Начало в {}", minutes_to_time(start)).into());
        } else {
            w.set_alert_title("Расписание".into());
            w.set_alert_subtitle("На сегодня пары закончились".into());
        }

        let streak = self.data.settings.streak_count.max(1);
        w.set_streak_title(format!("{streak} дней подряд").into());
        w.set_streak_subtitle("Личный рекорд".into());

        let weather = self.data.settings.cached_weather;
        if let Some(weather) = weather {
            let rain = if weather.is_rain { " · дождь" } else if weather.is_snow { " · снег" } else { "" };
            w.set_weather_text(
                format!("Алматы: {:.0}°C (ощущается {:.0}°C){rain}", weather.temperature_c, weather.apparent_temperature_c).into(),
            );
        } else {
            w.set_weather_text("Алматы: погода ещё не загружена".into());
        }

        let arrivals = fetch_departures("college", "home");
        w.set_transit_text(match (arrivals.first(), arrivals.get(1)) {
            (Some(a), Some(b)) => format!(
                "Транспорт: {} — {} мин · {} — {} мин",
                a.route, a.minutes, b.route, b.minutes
            )
            .into(),
            (Some(a), None) => format!("Транспорт: {} — {} мин", a.route, a.minutes).into(),
            _ => "Транспорт: данные недоступны".into(),
        });

        w.set_update_text(format!("Версия {}", env!("CARGO_PKG_VERSION")).into());
        w.set_busy(false);
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
            if screen == "add" {
                s.editing = None;
                s.window.set_edit_title("".into());
                s.window.set_edit_room("".into());
                s.window.set_edit_start("09:00".into());
                s.window.set_edit_end("10:30".into());
                s.window.set_edit_kind(0);
                s.window.set_screen("edit".into());
            } else {
                s.window.set_screen(screen.into());
            }
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
                s.window.set_edit_start(minutes_to_time(b.start_min).into());
                s.window.set_edit_end(minutes_to_time(b.end_min).into());
                s.window.set_edit_kind(kind_index(b.kind));
            }
            s.window.set_screen("edit".into());
        });

        let a = app.clone();
        w.on_edit_close(move || {
            let mut s = a.borrow_mut();
            s.editing = None;
            s.window.set_screen("schedule".into());
        });

        let a = app.clone();
        w.on_edit_save(move |title, room, start, end, kind| {
            let mut s = a.borrow_mut();
            let (Some(start_min), Some(end_min)) =
                (frayed_core::time_to_minutes(&start), frayed_core::time_to_minutes(&end))
            else {
                s.window.set_alert_subtitle("Неверный формат времени: HH:MM".into());
                return;
            };
            if end_min <= start_min {
                s.window.set_alert_subtitle("Конец должен быть позже начала".into());
                return;
            }
            let block = ClassBlock {
                title: title.trim().to_string(),
                room: room.trim().to_string(),
                start_min,
                end_min,
                kind: kind_from_index(kind),
            };
            let weekday = s.selected;
            let schedule = s.schedule();
            let mut blocks = effective_blocks(&schedule, &s.data.overrides, weekday);
            match s.editing.take() {
                Some(i) if i < blocks.len() => blocks[i] = block,
                _ => blocks.push(block),
            }
            if let Some(o) = s.data.overrides.days.iter_mut().find(|o| o.weekday == weekday) {
                o.cancelled = false;
                o.blocks = blocks;
            } else {
                s.data.overrides.days.push(frayed_core::DayOverride {
                    weekday,
                    cancelled: false,
                    blocks,
                });
            }
            let _ = storage::save(&s.data);
            s.window.set_screen("schedule".into());
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
        });

        let a = app.clone();
        w.on_preset_pressed(move |index| {
            let mut s = a.borrow_mut();
            if let Some(preset) = s.data.schedule_state.presets.get(index as usize) {
                s.data.schedule_state.active_preset_id = Some(preset.id.clone());
                let _ = storage::save(&s.data);
                s.refresh();
            }
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
    }
}

fn kind_index(kind: BlockKind) -> i32 {
    match kind {
        BlockKind::Lecture => 0,
        BlockKind::Lab => 1,
        BlockKind::Test => 2,
        BlockKind::Practice => 3,
        BlockKind::Break => 4,
        BlockKind::Custom => 5,
    }
}

fn kind_from_index(index: i32) -> BlockKind {
    match index {
        1 => BlockKind::Lab,
        2 => BlockKind::Test,
        3 => BlockKind::Practice,
        4 => BlockKind::Break,
        5 => BlockKind::Custom,
        _ => BlockKind::Lecture,
    }
}

fn strings<I: IntoIterator<Item = S>, S: Into<SharedString>>(items: I) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
}

fn block_view(block: &ClassBlock) -> BlockView {
    let (accent_rgb, tint_rgb): ((u8, u8, u8), (u8, u8, u8)) = match block.kind {
        BlockKind::Lecture => ((56, 189, 248), (56, 189, 248)),
        BlockKind::Lab => ((52, 211, 153), (52, 211, 153)),
        BlockKind::Test => ((129, 140, 248), (129, 140, 248)),
        BlockKind::Practice => ((251, 191, 36), (251, 191, 36)),
        BlockKind::Break => ((148, 163, 184), (148, 163, 184)),
        BlockKind::Custom => ((34, 197, 94), (34, 197, 94)),
    };
    let extra = match block.kind {
        BlockKind::Test => " · Тест",
        BlockKind::Lab => " · Лаборатория",
        _ => "",
    };
    BlockView {
        time: minutes_to_time(block.start_min).into(),
        title: block.title.clone().into(),
        subtitle: format!("{}{}", block.room, extra).into(),
        accent: Color::from_argb_u8(255, accent_rgb.0, accent_rgb.1, accent_rgb.2),
        tint: Color::from_argb_u8(26, tint_rgb.0, tint_rgb.1, tint_rgb.2),
    }
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

fn refresh_network(weak: slint::Weak<MainWindow>, mut data: AppData) {
    spawn(async move {
        let weather = OpenMeteoClient::new()
            .fetch_weather(frayed_net::ALMATY_LAT, frayed_net::ALMATY_LON)
            .await
            .ok();
        let release = GitHubReleaseClient::new().latest_release().await.ok();
        if let Some(weather) = weather {
            data.settings.cached_weather = Some(weather);
            let _ = storage::save(&data);
        }
        let update = match release {
            Some(r) if is_newer(env!("CARGO_PKG_VERSION"), &r.version) => {
                format!("Доступно обновление {}", r.tag_name)
            }
            Some(r) => format!("Актуальная версия {}", r.tag_name),
            None => "Релизы не найдены".to_string(),
        };
        slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                let saved = storage::load();
                if let Some(cached) = saved.settings.cached_weather {
                    let rain = if cached.is_rain { " · дождь" } else if cached.is_snow { " · снег" } else { "" };
                    w.set_weather_text(
                        format!("Алматы: {:.0}°C (ощущается {:.0}°C){rain}", cached.temperature_c, cached.apparent_temperature_c).into(),
                    );
                }
                w.set_update_text(update.into());
                w.set_busy(false);
            }
        })
        .ok();
    });
}

fn update_network(weak: slint::Weak<MainWindow>) {
    spawn(async move {
        let client = GitHubReleaseClient::new();
        let result = match client.latest_release().await {
            Ok(release) if is_newer(env!("CARGO_PKG_VERSION"), &release.version) => {
                let dir = storage::data_path()
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(std::env::temp_dir);
                let path = dir.join("frayed-update.apk");
                match client.download_release_apk(&release.apk_url, &path.to_string_lossy()).await {
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
        slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                w.set_update_text(result.into());
                w.set_busy(false);
            }
        })
        .ok();
    });
}

/// First launch gets a small starter schedule so the reference layout isn't empty.
/// ponytail: replace with import-from-file when editing UI lands.
fn seed_preset() -> frayed_core::GroupPreset {
    fn block(title: &str, room: &str, start: &str, end: &str, kind: BlockKind) -> ClassBlock {
        ClassBlock {
            title: title.into(),
            room: room.into(),
            start_min: frayed_core::time_to_minutes(start).unwrap_or(0),
            end_min: frayed_core::time_to_minutes(end).unwrap_or(0),
            kind,
        }
    }
    let mut schedule = Schedule::new();
    schedule.day_mut(2).blocks = vec![
        block("Geography", "Room 118", "09:00", "10:15", BlockKind::Lecture),
        block("English Lit", "Room B-204", "10:15", "11:30", BlockKind::Lecture),
        block("Lunch", "", "12:00", "13:00", BlockKind::Break),
        block("Chemistry", "Lab 3", "13:00", "14:30", BlockKind::Test),
        block("Soccer practice", "Field 3", "14:30", "16:00", BlockKind::Practice),
    ];
    schedule.day_mut(0).blocks = vec![block("Algebra", "Room 210", "09:00", "10:30", BlockKind::Lecture)];
    schedule.day_mut(1).blocks = vec![block("Physics", "Lab 1", "11:00", "12:30", BlockKind::Lab)];
    schedule.day_mut(3).blocks = vec![block("History", "Room 118", "09:00", "10:15", BlockKind::Lecture)];
    schedule.day_mut(4).blocks = vec![block("Economics", "Room 305", "10:00", "11:30", BlockKind::Practice)];
    frayed_core::GroupPreset { id: "main".into(), name: "Основная группа".into(), schedule }
}

#[cfg(target_os = "android")]
pub fn notify_startup(data: &AppData) {
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
        let count = blocks.len();
        crate::android::notify("Доброе утро", &format!("Сегодня {count} занятий"));
    }
    if let Some(start) = next_class_start(&blocks, now.minutes) {
        if start - now.minutes <= 45 {
            if let Some(b) = blocks.iter().find(|b| b.start_min == start) {
                crate::android::notify("Скоро пара", &format!("{} · {}", b.title, minutes_to_time(start)));
            }
        }
    }
}
