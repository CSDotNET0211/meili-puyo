use std::env;
use std::net::SocketAddr;

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use puyo_core::game::{
    Colour, GameState, Input, Pair, Placement, Rotation, StepError, route_to_placement,
};
use puyo_ai::mcts::{SearchConfig, SearchState, search};
use puyo_core::{Board, StepResult};
use serde::{Deserialize, Serialize};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLES_CSS: &str = include_str!("../web/styles.css");
const DEFAULT_ADDRESS: &str = "0.0.0.0:8791";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = env::var("PUYO_WEB_ADDR").unwrap_or_else(|_| DEFAULT_ADDRESS.to_owned());
    let address: SocketAddr = address.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let local_address = listener.local_addr()?;
    println!("Puyo SIMD Lab: http://{local_address}");
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn app() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(javascript))
        .route("/styles.css", get(stylesheet))
        .route("/api/health", get(health))
        .route("/api/simulate", post(simulate))
        .route("/api/game", post(game))
        .route("/api/search", post(search_game))
        .layer(DefaultBodyLimit::max(8 * 1024))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn index() -> impl IntoResponse {
    with_security_headers(Html(INDEX_HTML))
}

async fn javascript() -> impl IntoResponse {
    static_asset("text/javascript; charset=utf-8", APP_JS)
}

async fn stylesheet() -> impl IntoResponse {
    static_asset("text/css; charset=utf-8", STYLES_CSS)
}

fn static_asset(content_type: &'static str, body: &'static str) -> Response {
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    with_security_headers(response)
}

fn with_security_headers(response: impl IntoResponse) -> Response {
    let mut response = response.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'",
        ),
    );
    response
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        engine: "native-simd",
        sse2: true,
        bmi2: std::arch::is_x86_feature_detected!("bmi2"),
    })
}

async fn simulate(
    Json(request): Json<SimulationRequest>,
) -> Result<Json<SimulationResponse>, ApiError> {
    let initial = Board::parse(&request.board).map_err(|error| ApiError(error.to_string()))?;
    let labels = initial.chain_labels();
    let mut board = initial.clone();
    let mut frames = vec![SimulationFrame {
        chain: 0,
        board: board.to_string(),
        step: None,
    }];
    let mut score = 0u64;
    let mut colored_erased = 0u32;
    let mut garbage_erased = 0u32;
    let mut max_drop_distance = 0u8;

    for chain in 1..=u8::MAX {
        let Some(step) = board.resolve_step(chain) else {
            break;
        };
        score += u64::from(step.score);
        colored_erased += step.colored_erased;
        garbage_erased += step.garbage_erased;
        max_drop_distance = max_drop_distance.max(step.max_drop_distance);
        frames.push(SimulationFrame {
            chain,
            board: board.to_string(),
            step: Some(step.into()),
        });
    }

    Ok(Json(SimulationResponse {
        score,
        chains: (frames.len() - 1) as u8,
        colored_erased,
        garbage_erased,
        max_drop_distance,
        all_clear: board.is_empty(),
        labels: AuxiliaryLabelsResponse {
            layout: "floor-major-yx".to_owned(),
            vanish_step: labels.vanish_step.to_vec(),
            fall_distance: labels.fall_distance.to_vec(),
            component_size: labels.component_size.to_vec(),
        },
        frames,
    }))
}

async fn game(Json(request): Json<GameRequest>) -> Result<Json<GameResponse>, ApiError> {
    let board = Board::parse(&request.board).map_err(|error| ApiError(error.to_string()))?;
    let mut game = GameState::resume(
        request.seed,
        board,
        request.placements,
        request.maximum_chain,
    );
    let outcome = if let Some(action) = request.action {
        let placement = Placement {
            axis_column: action
                .column
                .checked_sub(1)
                .filter(|&column| column < 6)
                .ok_or_else(|| ApiError("column must be in 1..=6".to_owned()))?,
            rotation: parse_rotation(&action.rotation)?,
        };
        Some(game.step(placement).map_err(|error| match error {
            StepError::Terminal => ApiError("game is already terminal".to_owned()),
            StepError::IllegalPlacement => ApiError("placement is not reachable".to_owned()),
        })?)
    } else {
        None
    };
    let state = game.public_state();
    let legal_actions = game
        .legal_actions()
        .into_iter()
        .map(|action| GameActionResponse {
            column: action.placement.axis_column + 1,
            rotation: rotation_name(action.placement.rotation),
            route_frames: action.route_frames,
            route: action.route.into_iter().map(input_name).collect(),
        })
        .collect();
    Ok(Json(GameResponse {
        board: state.board.to_string(),
        pieces: state.pieces.map(pair_response),
        queue_position: state.queue_position,
        estimated_unseen_colour_counts: state.estimated_unseen_colour_counts,
        placements: state.placements,
        maximum_chain: state.maximum_chain,
        dead: state.dead,
        legal_actions,
        outcome: outcome.map(|value| GameOutcomeResponse {
            chains: value.simulation.chains,
            score: value.simulation.score,
            all_clear: value.simulation.all_clear,
            split_distance: value.split_distance,
            dead: value.dead,
        }),
    }))
}

async fn search_game(Json(request): Json<SearchRequest>) -> Result<Json<SearchResponse>, ApiError> {
    let board = Board::parse(&request.board).map_err(|error| ApiError(error.to_string()))?;
    let game = GameState::resume(
        request.seed,
        board,
        request.placements,
        request.maximum_chain,
    );
    let public = game.public_state();
    let result = search(
        SearchState::from(&public),
        SearchConfig {
            simulations: request.simulations.clamp(1, 100_000),
            max_depth: request.max_depth.clamp(1, 32),
            ..SearchConfig::default()
        },
    );
    let mut actions = result.actions;
    actions.sort_by_key(|action| std::cmp::Reverse(action.visits));
    Ok(Json(SearchResponse {
        root_value: result.root_value,
        nodes: result.nodes,
        actions: actions
            .into_iter()
            .map(|action| {
                let route = route_to_placement(&public.board, action.placement);
                SearchActionResponse {
                    column: action.placement.axis_column + 1,
                    rotation: rotation_name(action.placement.rotation),
                    visits: action.visits,
                    prior: action.prior,
                    mean_value: action.mean_value,
                    route_frames: route.as_ref().map_or(0, |route| route.route_frames),
                    route: route
                        .map(|route| route.route.into_iter().map(input_name).collect())
                        .unwrap_or_default(),
                }
            })
            .collect(),
    }))
}

fn parse_rotation(value: &str) -> Result<Rotation, ApiError> {
    match value.to_ascii_lowercase().as_str() {
        "right" | "r" => Ok(Rotation::Right),
        "down" | "d" => Ok(Rotation::Down),
        "left" | "l" => Ok(Rotation::Left),
        "up" | "u" => Ok(Rotation::Up),
        _ => Err(ApiError("rotation must be right/down/left/up".to_owned())),
    }
}

const fn rotation_name(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::Right => "right",
        Rotation::Down => "down",
        Rotation::Left => "left",
        Rotation::Up => "up",
    }
}

const fn input_name(input: Input) -> &'static str {
    match input {
        Input::Left => "left",
        Input::Right => "right",
        Input::Down => "down",
        Input::RotateCw => "rotate_cw",
        Input::RotateCcw => "rotate_ccw",
        Input::Rotate180 => "rotate_180",
        Input::HardDrop => "hard_drop",
    }
}

const fn colour_name(colour: Colour) -> &'static str {
    match colour {
        Colour::Red => "red",
        Colour::Green => "green",
        Colour::Blue => "blue",
        Colour::Yellow => "yellow",
    }
}

const fn pair_response(pair: Pair) -> PairResponse {
    PairResponse {
        axis: colour_name(pair.axis),
        child: colour_name(pair.child),
    }
}

#[derive(Debug, Deserialize)]
struct SimulationRequest {
    board: String,
}

#[derive(Debug, Deserialize)]
struct GameRequest {
    board: String,
    #[serde(default)]
    seed: u32,
    #[serde(default)]
    placements: u32,
    #[serde(default)]
    maximum_chain: u8,
    action: Option<GameActionRequest>,
}

#[derive(Debug, Deserialize)]
struct SearchRequest {
    board: String,
    #[serde(default)]
    seed: u32,
    #[serde(default)]
    placements: u32,
    #[serde(default)]
    maximum_chain: u8,
    #[serde(default = "default_simulations")]
    simulations: u32,
    #[serde(default = "default_search_depth")]
    max_depth: u8,
}

const fn default_simulations() -> u32 {
    800
}

const fn default_search_depth() -> u8 {
    8
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    root_value: f32,
    nodes: usize,
    actions: Vec<SearchActionResponse>,
}

#[derive(Debug, Serialize)]
struct SearchActionResponse {
    column: u8,
    rotation: &'static str,
    visits: u32,
    prior: f32,
    mean_value: f32,
    route_frames: u16,
    route: Vec<&'static str>,
}

#[derive(Debug, Deserialize)]
struct GameActionRequest {
    column: u8,
    rotation: String,
}

#[derive(Debug, Serialize)]
struct GameResponse {
    board: String,
    pieces: [PairResponse; 3],
    queue_position: u8,
    estimated_unseen_colour_counts: [u16; 4],
    placements: u32,
    maximum_chain: u8,
    dead: bool,
    legal_actions: Vec<GameActionResponse>,
    outcome: Option<GameOutcomeResponse>,
}

#[derive(Debug, Serialize)]
struct PairResponse {
    axis: &'static str,
    child: &'static str,
}

#[derive(Debug, Serialize)]
struct GameActionResponse {
    column: u8,
    rotation: &'static str,
    route_frames: u16,
    route: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct GameOutcomeResponse {
    chains: u8,
    score: u64,
    all_clear: bool,
    split_distance: u8,
    dead: bool,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    engine: &'static str,
    sse2: bool,
    bmi2: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimulationResponse {
    score: u64,
    chains: u8,
    colored_erased: u32,
    garbage_erased: u32,
    max_drop_distance: u8,
    all_clear: bool,
    labels: AuxiliaryLabelsResponse,
    frames: Vec<SimulationFrame>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AuxiliaryLabelsResponse {
    layout: String,
    vanish_step: Vec<u8>,
    fall_distance: Vec<u8>,
    component_size: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimulationFrame {
    chain: u8,
    board: String,
    step: Option<StepResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StepResponse {
    score: u32,
    colored_erased: u32,
    garbage_erased: u32,
    colors_erased: u8,
    group_bonus: u32,
    waste: u32,
    max_drop_distance: u8,
    erased_cells: Vec<CellPosition>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CellPosition {
    row: u8,
    column: u8,
}

impl From<StepResult> for StepResponse {
    fn from(step: StepResult) -> Self {
        let erased_cells = (0..13)
            .flat_map(|y| {
                (0..6).filter_map(move |x| {
                    let bit = 1u128 << ((x + 1) * 16 + y + 1);
                    (step.erased_mask & bit != 0).then_some(CellPosition {
                        row: 12 - y as u8,
                        column: x as u8,
                    })
                })
            })
            .collect();
        Self {
            score: step.score,
            colored_erased: step.colored_erased,
            garbage_erased: step.garbage_erased,
            colors_erased: step.colors_erased,
            group_bonus: step.group_bonus,
            waste: step.waste,
            max_drop_distance: step.max_drop_distance,
            erased_cells,
        }
    }
}

#[derive(Debug)]
struct ApiError(String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse { error: self.0 }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const ONE_CHAIN: &str = "....../....../....../....../....../....../....../....../....../....../....../....../RRRR..";

    #[tokio::test]
    async fn health_reports_native_engine() {
        let response = app()
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let health: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(health["status"], "ok");
        assert_eq!(health["engine"], "native-simd");
    }

    #[tokio::test]
    async fn simulation_endpoint_returns_frames() {
        let body = serde_json::json!({ "board": ONE_CHAIN }).to_string();
        let response = app()
            .oneshot(
                Request::post("/api/simulate")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let result: SimulationResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.chains, 1);
        assert_eq!(result.score, 40);
        assert_eq!(result.frames.len(), 2);
        assert!(result.all_clear);
        assert_eq!(result.labels.vanish_step.len(), 78);
        assert_eq!(result.labels.component_size.iter().copied().max(), Some(4));
        assert_eq!(
            result.frames[1].step.as_ref().unwrap().erased_cells.len(),
            4
        );
    }

    #[tokio::test]
    async fn floating_board_is_rejected() {
        let floating = "....../....../....../....../....../....../....../....../....../....../R...../....../......";
        let body = serde_json::json!({ "board": floating }).to_string();
        let response = app()
            .oneshot(
                Request::post("/api/simulate")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn game_endpoint_exposes_only_current_and_next_two_with_legal_routes() {
        let empty = "....../....../....../....../....../....../....../....../....../....../....../....../......";
        let body = serde_json::json!({ "board": empty, "seed": 0 }).to_string();
        let response = app()
            .oneshot(
                Request::post("/api/game")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["pieces"].as_array().unwrap().len(), 3);
        assert_eq!(value["legal_actions"].as_array().unwrap().len(), 22);
        assert!(value.get("queue").is_none());
        assert!(
            value["legal_actions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|action| {
                    action["route"].as_array().unwrap().last().unwrap() == "hard_drop"
                })
        );
    }

    #[tokio::test]
    async fn game_endpoint_applies_a_placement_and_advances_next() {
        let empty = "....../....../....../....../....../....../....../....../....../....../....../....../......";
        let body = serde_json::json!({
            "board": empty,
            "seed": 0,
            "action": { "column": 1, "rotation": "up" }
        })
        .to_string();
        let response = app()
            .oneshot(
                Request::post("/api/game")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["placements"], 1);
        assert_eq!(value["queue_position"], 1);
        assert_eq!(value["outcome"]["chains"], 0);
    }

    #[tokio::test]
    async fn search_endpoint_returns_an_exact_root_visit_distribution() {
        let empty = "....../....../....../....../....../....../....../....../....../....../....../....../......";
        let body = serde_json::json!({
            "board": empty,
            "seed": 3,
            "simulations": 128,
            "max_depth": 4
        })
        .to_string();
        let response = app()
            .oneshot(
                Request::post("/api/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let actions = value["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 22);
        assert_eq!(
            actions
                .iter()
                .map(|action| action["visits"].as_u64().unwrap())
                .sum::<u64>(),
            128
        );
        assert!(value["nodes"].as_u64().unwrap() > 22);
    }
}
