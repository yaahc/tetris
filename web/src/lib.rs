mod fps;
mod graphics;
mod input;

use log::info;
use serde_json::from_str;
use std::sync::mpsc::{self, channel, Receiver};
use tetris::game::Lookahead;
use tetris::sound::{NullSink, Sink, SoundPlayer};
use tetris::{Config, Event, Game, GameState, SettingEvent};
use tetrizz::eval::Eval;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, HtmlDivElement, HtmlInputElement};
use web_time::Instant;

use crate::fps::FPSCounter;
use crate::graphics::Skin;

#[wasm_bindgen]
pub async fn main() -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    wasm_logger::init(wasm_logger::Config::new(log::Level::Debug));
    info!("wasm blob initialized, running main...");
    let window = web_sys::window().unwrap();
    let doc = window.document().unwrap();
    let default_skin = "https://i.imgur.com/zjItrsg.png";
    let skin = graphics::load_skin(default_skin).await?;
    let board = doc.get_element_by_id("board").unwrap().dyn_into::<web_sys::HtmlCanvasElement>()?;
    let hold = doc.get_element_by_id("hold").unwrap().dyn_into::<web_sys::HtmlCanvasElement>()?;
    let queue = doc.get_element_by_id("queue").unwrap().dyn_into::<HtmlCanvasElement>()?;
    let timer_div = doc.get_element_by_id("timer").unwrap().dyn_into::<HtmlDivElement>()?;
    let fps_div = doc.get_element_by_id("fps").unwrap().dyn_into::<HtmlDivElement>()?;
    let spins_div = doc.get_element_by_id("spins").unwrap().dyn_into::<HtmlDivElement>()?;
    let right_info_div =
        doc.get_element_by_id("right-info").unwrap().dyn_into::<HtmlDivElement>()?;
    let config = load_config();
    let start = Instant::now();
    let timelimit = std::time::Duration::from_secs(20 * 60);
    let (tx, rx) = channel();
    init_menu_callbacks(tx.clone());
    input::init_input_handlers(tx)?;
    let (mut raf_loop, _canceler) = wasm_repeated_animation_frame::RafLoop::new();
    let mut fps = fps::FPSCounter::new();
    let mut game = Game::new(config);
    // game.mode = tetris::Mode::Sprint { target_lines: 40 };
    game.mode = tetris::Mode::TrainingLab {
        search: false,
        lookahead: Some(Lookahead::new(3, 30)),
        // lookahead: None,
        mino_mode: true,
    };
    info!("starting event loop, why won't you work!?");
    info!("mode: {:?}", game.mode);
    let sound = SoundPlayer::<NullSink>::default();
    game.start(None, &sound);
    let mut new_piece = false;
    let eval = &tetrizz::eval::Eval::new(
        -79.400375,
        -55.564907,
        -125.680145,
        -170.41902,
        10.167948,
        -172.78625,
        -478.7291,
        86.84883,
        368.89203,
        272.57874,
        28.938646,
        -104.59018,
        -496.8832,
        458.29822,
    );

    // TODO: eventually we wanna go back to separate event loops for inputs/drawing/timers,
    // but for now this makes it easy to share game state between those
    let raf_fut = async {
        loop {
            raf_loop.next().await;
            run_loop(
                &mut game,
                &board,
                &queue,
                &hold,
                &skin,
                &mut fps,
                &timer_div,
                &fps_div,
                &right_info_div,
                &spins_div,
                &rx,
                &sound,
                eval,
                &mut new_piece,
            );

            // TODO blank the screen or something
            let now = Instant::now();
            if now > start + timelimit {
                timer_div.set_text_content(Some("DONE!!!!! Gaming Time is over. Please Stretch"));
                break;
            }
        }
    };
    raf_fut.await;
    Ok(())
}

fn load_config() -> Config {
    let window = web_sys::window().unwrap();
    let storage = window.local_storage().unwrap().unwrap();

    let default_config = Config {
        das: 6,
        arr: 0,
        gravity: Some(60),
        soft_drop: 1,
        lock_delay: (60, 300, 1200),
        ghost: true,
    };

    let das =
        storage.get("das").unwrap().map(|s| from_str(&s).unwrap()).unwrap_or(default_config.das);
    let arr =
        storage.get("arr").unwrap().map(|s| from_str(&s).unwrap()).unwrap_or(default_config.arr);
    let gravity = storage
        .get("gravity")
        .unwrap()
        .map(|s| from_str(&s).unwrap())
        .unwrap_or(default_config.gravity);
    let gravity = match gravity {
        Some(0) => None,
        gravity => gravity,
    };
    let soft_drop = storage
        .get("soft-drop")
        .unwrap()
        .map(|s| from_str(&s).unwrap())
        .unwrap_or(default_config.soft_drop);
    let lock_delay = storage
        .get("lock-delay")
        .unwrap()
        .map(|s| from_str(&s).unwrap())
        .unwrap_or(default_config.lock_delay);
    let ghost = storage
        .get("ghost")
        .unwrap()
        .map(|s| from_str(&s).unwrap())
        .unwrap_or(default_config.ghost);

    Config { das, arr, gravity, soft_drop, lock_delay, ghost }
}

fn init_menu_callbacks(events: mpsc::Sender<Event>) {
    let window = web_sys::window().unwrap();
    let storage = window.local_storage().unwrap().unwrap();
    let doc = window.document().unwrap();
    let handler = move |event: web_sys::Event| {
        let element = event.target().unwrap().value_of().dyn_into::<HtmlInputElement>().unwrap();
        let value = element.value();
        let name = element.id();
        storage.set(&name, &value).unwrap();
        let event = match name.as_str() {
            "das" => Event::Setting(SettingEvent::Das(value.parse().unwrap())),
            "arr" => Event::Setting(SettingEvent::Arr(value.parse().unwrap())),
            "gravity" => {
                let value: u16 = value.parse().unwrap();
                let value = if value == 0 { None } else { Some(value) };
                Event::Setting(SettingEvent::Gravity(value))
            }
            "soft-drop" => Event::Setting(SettingEvent::SoftDrop(value.parse().unwrap())),
            _ => todo!(),
        };
        events.send(event).unwrap();
    };
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(_)>);
    let das = doc.get_element_by_id("das").unwrap().dyn_into::<HtmlInputElement>().unwrap();
    das.set_onchange(Some(closure.as_ref().unchecked_ref()));
    let arr = doc.get_element_by_id("arr").unwrap().dyn_into::<HtmlInputElement>().unwrap();
    arr.set_onchange(Some(closure.as_ref().unchecked_ref()));
    let gravity = doc.get_element_by_id("gravity").unwrap().dyn_into::<HtmlInputElement>().unwrap();
    gravity.set_onchange(Some(closure.as_ref().unchecked_ref()));
    let soft_drop =
        doc.get_element_by_id("soft-drop").unwrap().dyn_into::<HtmlInputElement>().unwrap();
    soft_drop.set_onchange(Some(closure.as_ref().unchecked_ref()));
    std::mem::forget(closure);
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    game: &mut Game,
    board: &HtmlCanvasElement,
    queue: &HtmlCanvasElement,
    hold: &HtmlCanvasElement,
    skin: &Skin,
    fps_counter: &mut FPSCounter,
    timer: &HtmlDivElement,
    fps: &HtmlDivElement,
    line_count: &HtmlDivElement,
    spins: &HtmlDivElement,
    rx: &Receiver<Event>,
    sound: &SoundPlayer<impl Sink>,
    eval: &Eval,
    new_piece: &mut bool,
) {
    let now = Instant::now();
    fps.set_text_content(Some(&format!("fps: {}", fps_counter.tick(now))));

    let t = if let Some(start_time) = game.start_time {
        game.end_time.unwrap_or(now).duration_since(start_time).as_secs_f64()
    } else {
        0.0
    };
    timer.set_text_content(Some(&format!("{t:.2}")));

    if let tetris::Mode::Sprint { target_lines: target } = game.mode {
        line_count.set_text_content(Some(&format!("{}", target.saturating_sub(game.lines))));
    }
    while let Ok(e) = rx.try_recv() {
        use tetris::{Event::*, GameState::*, InputEvent::*};
        if let Input(Restart) = e {
            game.start(None, sound);
            break;
        }
        info!("search enabled: {}, new_piece: {}", game.mode.search_enabled(), new_piece);
        if game.mode.search_enabled() && *new_piece {
            // call search algorithm
            log::info!("upcoming: {:?}", game.upcomming);
            log::info!("hold: {:?}", game.hold);
            log::info!("current: {:?}", game.current);
            let (tetrizz_game, queue) = game.as_tetrizz_game_and_queue();
            log::info!("queue: {queue:?}");
            log::info!("hold: {:?}", tetrizz_game.hold);
            let search_loc = tetrizz::movegen::movegen(&tetrizz_game, queue[0]);
            let heap = tetrizz::beam_search::search_results(
                &tetrizz_game,
                &search_loc,
                queue,
                eval,
                7,
                3000,
            );
            let mut spins = vec![];
            for node in heap.iter() {
                for (m, placement_info) in node.moves.iter() {
                    if placement_info.lines_cleared > 0 {
                        if m.spun {
                            spins.push(node.clone());
                        }
                        break;
                    }
                }
            }
            spins.sort_by_key(|s| s.score);
            spins.sort_by_key(|s| s.moves.iter().take_while(|m| !m.1.spin).count());
            game.spins = spins;
            *new_piece = false;
        }
        if game.state == Running
            || game.state == Startup
                && matches!(e, Input(PressLeft | PressRight | ReleaseLeft | ReleaseRight))
        {
            *new_piece |= game.handle(e, now, sound);
        }
    }
    if game.state == GameState::Done {
        game.timers.clear();
    }
    while let Some(&(t, timer_event)) = game.timers.front() {
        if t < now {
            game.timers.pop_front();
            game.handle(Event::Timer(timer_event), now, sound);
        } else {
            break;
        }
    }

    graphics::draw_board(game, board, skin, t).unwrap();
    // could do these only when needed instead of every frame if we wanted
    graphics::draw_queue(game, queue, skin, 5).unwrap();
    graphics::draw_hold(game, hold, skin).unwrap();

    let spin_text = game.display_spins().to_string();
    info!("spins: {spin_text}");
    spins.set_text_content(Some(&spin_text));
}
