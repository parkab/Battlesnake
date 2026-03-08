use actix_web::{web, App, HttpResponse, HttpServer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

mod api;
mod bitboard;
mod board;
mod heuristic;
mod minimax;
mod opponent;
mod simulator;
mod strategy;
mod types;

use api::{ApiGameState, InfoResponse, MoveResponse};
use minimax::TranspositionTable;
use opponent::GameTracker;

struct AppState {
    trackers: Arc<Mutex<HashMap<String, GameTracker>>>,
    tt: Arc<Mutex<TranspositionTable>>,
}

async fn handle_info() -> HttpResponse {
    HttpResponse::Ok().json(InfoResponse {
        apiversion: "1".to_string(),
        author: "the_snakiest".to_string(),
        color: "#433fff".to_string(),
        head: "gamer".to_string(),
        tail: "rbc-necktie".to_string(),
        version: "2.0.0".to_string(),
    })
}

async fn handle_start(
    app: web::Data<AppState>,
    gs: web::Json<ApiGameState>,
) -> HttpResponse {
    let game_id = gs.game.id.clone();
    let num_snakes = gs.board.snakes.len();
    let board_size = format!("{}x{}", gs.board.width, gs.board.height);

    {
        let mut trackers = app.trackers.lock().unwrap();
        trackers.insert(game_id.clone(), GameTracker::new(&gs));
    }

    println!("[START] Game {} | {} snakes | {} board", game_id, num_snakes, board_size);
    HttpResponse::Ok().json(serde_json::json!({"ok": true}))
}

async fn handle_move(
    app: web::Data<AppState>,
    gs: web::Json<ApiGameState>,
) -> HttpResponse {
    let start = std::time::Instant::now();

    let tracker = {
        let trackers = app.trackers.lock().unwrap();
        trackers.get(&gs.game.id).cloned()
    };

    let mut tt = app.tt.lock().unwrap();

    let result: MoveResponse = strategy::decide_move(&gs, tracker.as_ref(), &mut tt);

    drop(tt);

    let elapsed = start.elapsed().as_millis();
    println!(
        "[MOVE T{}] {} | hp:{} len:{} alive:{} | {}ms",
        gs.turn,
        result.direction,
        gs.you.health,
        gs.you.length,
        gs.board.snakes.len(),
        elapsed
    );

    {
        let mut trackers = app.trackers.lock().unwrap();
        if let Some(tracker) = trackers.get_mut(&gs.game.id) {
            tracker.record_turn(&gs);
            let board = board::Board::new(&gs);
            let (my_area, _total) = board.voronoi_area();
            tracker.record_voronoi(my_area);
        }
    }

    HttpResponse::Ok().json(result)
}

async fn handle_end(
    app: web::Data<AppState>,
    gs: web::Json<ApiGameState>,
) -> HttpResponse {
    let game_id = gs.game.id.clone();
    let turns = gs.turn;

    {
        let mut trackers = app.trackers.lock().unwrap();
        trackers.remove(&game_id);
    }

    println!("[END] Game {} after {} turns", game_id, turns);
    HttpResponse::Ok().json(serde_json::json!({"ok": true}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);

    let app_state = web::Data::new(AppState {
        trackers: Arc::new(Mutex::new(HashMap::new())),
        tt: Arc::new(Mutex::new(TranspositionTable::new())),
    });

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║        APEX PREDATOR Battlesnake v2.0.0 (Rust)          ║");
    println!("║  Bitboard flood fill | Alpha-beta + TT | Phase blending  ║");
    println!("║  Port: {:<51}║", port);
    println!("╚══════════════════════════════════════════════════════════╝");

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .app_data(web::JsonConfig::default().limit(1_048_576))
            .route("/", web::get().to(handle_info))
            .route("/start", web::post().to(handle_start))
            .route("/move", web::post().to(handle_move))
            .route("/end", web::post().to(handle_end))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
