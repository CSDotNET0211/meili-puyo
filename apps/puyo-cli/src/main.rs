use std::env;
use std::fs::File;
use std::hint::black_box;
use std::io::{self, BufWriter, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use puyo_ai::mcts::{
    BaselineEvaluator, Evaluator, SearchConfig, SearchState, search, search_with_evaluator,
};
use puyo_core::game::{
    DEATH_X, DEATH_Y, GameState, Pair, Placement, Rotation, legal_placements,
    legal_placements_compact, place_pair, route_to_placement,
};
use puyo_core::{Board, Cell, HEIGHT, WIDTH};
use serde::Serialize;

mod play;

const DEMO: &str =
    "....../....../....../....../....../....../....../....../....../....../....../RR..../RRGG..";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    match command {
        "play" => {
            let seed = parse_arg(&args, 1, 0u32)?;
            let simulations = parse_arg(&args, 2, 0u32)?;
            play::run(seed, simulations)
        }
        "simulate" => {
            let board_text = match args.get(1).map(String::as_str) {
                Some("--board") => args.get(2).ok_or("simulate --board requires a value")?.clone(),
                Some("--stdin") | None => {
                    if args.get(1).is_some() {
                        let mut input = String::new();
                        io::stdin().read_to_string(&mut input)?;
                        input
                    } else {
                        DEMO.to_owned()
                    }
                }
                Some(other) => {
                    return Err(format!("unknown argument {other:?}; use --help").into());
                }
            };
            simulate(&board_text)
        }
        "suggest" => {
            let seed = parse_arg(&args, 1, 0u32)?;
            let simulations = parse_arg(&args, 2, 800u32)?;
            suggest(seed, simulations)
        }
        "bench" => {
            let iterations = parse_arg(&args, 1, 5_000_000usize)?;
            benchmark(iterations);
            Ok(())
        }
        "mcts-bench" => {
            let searches = parse_arg(&args, 1, 100usize)?;
            let simulations = parse_arg(&args, 2, 800u32)?;
            benchmark_mcts(searches, simulations);
            Ok(())
        }
        "selfplay" => {
            let output = args.get(1).ok_or("selfplay requires an output path")?;
            let games = parse_arg(&args, 2, 10usize)?;
            let simulations = parse_arg(&args, 3, 800u32)?;
            let max_moves = parse_arg(&args, 4, 128usize)?;
            generate_selfplay(output, games, simulations, max_moves)
        }
        "--help" | "-h" | "help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command {other:?}; use --help").into()),
    }
}

fn print_help() {
    println!(
        "puyo-cli — Puyo Puyo simulator & AI\n\
         \n\
         USAGE:\n\
         \x20   puyo-cli play [SEED] [SIMULATIONS]     Interactive CUI play (SIMULATIONS>0 enables AI hints)\n\
         \x20   puyo-cli simulate [--board ROWS|--stdin] Resolve chains on a board\n\
         \x20   puyo-cli suggest [SEED] [SIMULATIONS]  Print MCTS suggestion for a fresh game\n\
         \x20   puyo-cli bench [ITERATIONS]            Chain simulation benchmark\n\
         \x20   puyo-cli mcts-bench [SEARCHES] [SIMS]  MCTS benchmark\n\
         \x20   puyo-cli selfplay OUT.jsonl [GAMES] [SIMS] [MAX_MOVES]\n\
         \n\
         Board input is {HEIGHT} top-to-bottom rows of {WIDTH} cells.\n\
         Use . for empty, O for garbage, and R/G/B/Y for colors.\n\
         \n\
         PLAY KEYS:\n\
         \x20   ←/→ or h/l  move    ↑/k  rotate CW    ↓/j  soft drop\n\
         \x20   z  rotate CCW       x  rotate CW      c  rotate 180\n\
         \x20   space  hard drop    q  quit"
    );
}

fn simulate(board_text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut board = Board::parse(board_text)?;
    println!("before:\n{board}\n");
    let result = board.simulate();
    println!(
        "chains={} score={} all_clear={}",
        result.chains(),
        result.score,
        result.all_clear
    );
    for step in &result.steps {
        println!(
            "  chain {}: score={} colors={} colored={} garbage={} max_drop={}",
            step.chain_index,
            step.score,
            step.colors_erased,
            step.colored_erased,
            step.garbage_erased,
            step.max_drop_distance
        );
    }
    println!("\nafter:\n{board}");
    Ok(())
}

fn suggest(seed: u32, simulations: u32) -> Result<(), Box<dyn std::error::Error>> {
    let game = GameState::new(seed);
    let public = game.public_state();
    println!("board:\n{}\n", public.board);
    println!(
        "pieces: {:?}{:?} / {:?}{:?} / {:?}{:?}",
        public.pieces[0].axis,
        public.pieces[0].child,
        public.pieces[1].axis,
        public.pieces[1].child,
        public.pieces[2].axis,
        public.pieces[2].child,
    );
    let config = SearchConfig {
        simulations,
        ..SearchConfig::default()
    };
    let started = Instant::now();
    let result = search(SearchState::from(&public), config);
    let elapsed = started.elapsed();
    println!("\nMCTS ({} simulations, {:.1?}):", simulations, elapsed);
    for action in &result.actions {
        println!(
            "  col={} rot={:?} visits={} prior={:.3} mean={:.3}",
            action.placement.axis_column,
            action.placement.rotation,
            action.visits,
            action.prior,
            action.mean_value
        );
    }
    if let Some(best) = result.best() {
        println!(
            "\nbest: column {} rotation {:?}",
            best.placement.axis_column, best.placement.rotation
        );
    }
    Ok(())
}

fn parse_arg<T>(args: &[String], index: usize, default: T) -> Result<T, T::Err>
where
    T: std::str::FromStr,
{
    args.get(index)
        .map(|value| value.parse())
        .transpose()
        .map(|value| value.unwrap_or(default))
}

const LABEL_HORIZONS: [usize; 4] = [8, 16, 32, 64];

#[derive(Serialize)]
struct SelfplayAction {
    column: u8,
    rotation: u8,
    axis_colour: u8,
    child_colour: u8,
    route_frames: u16,
    visits: u32,
    prior: f32,
    q_value: f32,
    after_board: Vec<u8>,
    immediate_chain: u8,
    dead: bool,
}

#[derive(Serialize)]
struct SelfplayRecord {
    schema: u8,
    game: usize,
    ply: usize,
    board: Vec<u8>,
    pieces: [[u8; 2]; 3],
    actions: Vec<SelfplayAction>,
    root_value: f32,
    root_evaluation: f32,
    survival: [bool; 4],
    survival_valid: [bool; 4],
    max_chain: [u8; 4],
    chain_valid: [bool; 4],
}

struct PendingRecord {
    board: Board,
    pieces: [Pair; 3],
    actions: Vec<SelfplayAction>,
    root_value: f32,
    root_evaluation: f32,
}

fn generate_selfplay(
    output: &str,
    games: usize,
    simulations: u32,
    max_moves: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    generate_selfplay_with(output, games, simulations, max_moves, &mut BaselineEvaluator)
}

fn generate_selfplay_with<E: Evaluator>(
    output: &str,
    games: usize,
    simulations: u32,
    max_moves: usize,
    evaluator: &mut E,
) -> Result<(), Box<dyn std::error::Error>> {
    if games == 0 || simulations == 0 || max_moves == 0 {
        return Err("games, simulations, and max moves must be positive".into());
    }
    let started = Instant::now();
    let mut writer = BufWriter::new(File::create(output)?);
    let config = SearchConfig {
        simulations,
        ..SearchConfig::default()
    };
    let mut total_positions = 0usize;
    for game_index in 0..games {
        let seed = game_index as u32;
        let mut game = GameState::new(seed);
        let mut random = 0x9e37_79b9_7f4a_7c15u64 ^ u64::from(seed);
        let mut pending = Vec::with_capacity(max_moves);
        let mut chains = Vec::with_capacity(max_moves);
        for _ in 0..max_moves {
            let public = game.public_state();
            if public.dead {
                break;
            }
            let result = search_with_evaluator(SearchState::from(&public), config, evaluator);
            if result.actions.is_empty() {
                break;
            }
            let actions = result
                .actions
                .iter()
                .map(|action| {
                    selfplay_action(
                        &public.board,
                        public.pieces[0],
                        action.placement,
                        action.visits,
                        action.prior,
                        action.mean_value,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let selected = sample_visit_action(&result.actions, &mut random);
            pending.push(PendingRecord {
                board: public.board,
                pieces: public.pieces,
                actions,
                root_value: result.root_value,
                root_evaluation: result.root_evaluation,
            });
            let outcome = game
                .step_fast(selected)
                .map_err(|error| format!("selfplay produced an illegal action: {error:?}"))?;
            chains.push(outcome.simulation.chains);
            if outcome.dead {
                break;
            }
        }
        let terminal = game.public_state().dead;
        for (ply, position) in pending.into_iter().enumerate() {
            let available = chains.len().saturating_sub(ply);
            let survival = std::array::from_fn(|index| available >= LABEL_HORIZONS[index]);
            let valid = std::array::from_fn(|index| terminal || available >= LABEL_HORIZONS[index]);
            let max_chain = std::array::from_fn(|index| {
                chains[ply..ply + available.min(LABEL_HORIZONS[index])]
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(0)
            });
            let record = SelfplayRecord {
                schema: 2,
                game: game_index,
                ply,
                board: encode_board(&position.board),
                pieces: position.pieces.map(|pair| [pair.axis as u8, pair.child as u8]),
                actions: position.actions,
                root_value: position.root_value,
                root_evaluation: position.root_evaluation,
                survival,
                survival_valid: valid,
                max_chain,
                chain_valid: valid,
            };
            serde_json::to_writer(&mut writer, &record)?;
            writer.write_all(b"\n")?;
            total_positions += 1;
        }
    }
    writer.flush()?;
    let elapsed = started.elapsed();
    println!(
        "selfplay games={games} positions={total_positions} simulations={simulations} elapsed={elapsed:.3?} positions_per_second={:.1} output={output}",
        total_positions as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}

fn selfplay_action(
    board: &Board,
    pair: Pair,
    placement: Placement,
    visits: u32,
    prior: f32,
    q_value: f32,
) -> Result<SelfplayAction, Box<dyn std::error::Error>> {
    let route = route_to_placement(board, placement).ok_or("legal placement has no route")?;
    let mut after_board = board.clone();
    place_pair(&mut after_board, pair, placement).ok_or("legal placement cannot be placed")?;
    let immediate_chain = after_board.simulate_chain_count();
    let dead = after_board.get(DEATH_X, DEATH_Y) != Cell::Empty;
    Ok(SelfplayAction {
        column: placement.axis_column,
        rotation: rotation_id(placement.rotation),
        axis_colour: pair.axis as u8,
        child_colour: pair.child as u8,
        route_frames: route.route_frames,
        visits,
        prior,
        q_value,
        after_board: encode_board(&after_board),
        immediate_chain,
        dead,
    })
}

const fn rotation_id(rotation: Rotation) -> u8 {
    match rotation {
        Rotation::Right => 0,
        Rotation::Down => 1,
        Rotation::Left => 2,
        Rotation::Up => 3,
    }
}

fn sample_visit_action(actions: &[puyo_ai::mcts::RootAction], random: &mut u64) -> Placement {
    let total = actions
        .iter()
        .map(|action| u64::from(action.visits))
        .sum::<u64>();
    let mut choice = next_random(random) % total.max(1);
    for action in actions {
        if choice < u64::from(action.visits) {
            return action.placement;
        }
        choice = choice.saturating_sub(u64::from(action.visits));
    }
    actions[actions.len() - 1].placement
}

fn encode_board(board: &Board) -> Vec<u8> {
    let mut cells = Vec::with_capacity(WIDTH * HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            cells.push(match board.get(x, y) {
                Cell::Empty => 0,
                Cell::Garbage => 1,
                Cell::Red => 2,
                Cell::Green => 3,
                Cell::Blue => 4,
                Cell::Yellow => 5,
            });
        }
    }
    cells
}

#[inline(never)]
fn benchmark_mcts(searches: usize, simulations: u32) {
    let states = (0..searches)
        .map(|seed| SearchState::from(&GameState::new(seed as u32).public_state()))
        .collect::<Vec<_>>();
    let config = SearchConfig {
        simulations,
        ..SearchConfig::default()
    };
    let started = Instant::now();
    let mut nodes = 0usize;
    let mut visits = 0u64;
    for state in states {
        let result = black_box(search(state, config));
        nodes += result.nodes;
        visits += result
            .actions
            .iter()
            .map(|action| u64::from(action.visits))
            .sum::<u64>();
    }
    let elapsed = started.elapsed();
    println!(
        "searches={searches} simulations={simulations} elapsed={elapsed:.3?} search_ms={:.3} simulations_per_second={:.0} mean_nodes={:.1} checksum={visits}",
        elapsed.as_secs_f64() * 1e3 / searches as f64,
        searches as f64 * f64::from(simulations) / elapsed.as_secs_f64(),
        nodes as f64 / searches as f64,
    );
}

#[inline(never)]
fn benchmark(iterations: usize) {
    let mut templates = Vec::with_capacity(256);
    let mut random = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..256 {
        let mut board = Board::empty();
        for x in 0..WIDTH {
            let height = (next_random(&mut random) % (HEIGHT as u64 + 1)) as usize;
            for y in 0..height {
                let cell = match next_random(&mut random) % 5 {
                    0 => Cell::Garbage,
                    1 => Cell::Red,
                    2 => Cell::Green,
                    3 => Cell::Blue,
                    _ => Cell::Yellow,
                };
                board.set(x, y, cell);
            }
        }
        templates.push(board);
    }

    let copy_started = Instant::now();
    for index in 0..iterations {
        let board = templates[index & 255].clone();
        black_box(&board);
    }
    let copy_elapsed = copy_started.elapsed();

    let started = Instant::now();
    let mut chains = 0usize;
    let mut score = 0u64;
    for index in 0..iterations {
        let mut board = templates[index & 255].clone();
        let result = black_box(&mut board).simulate_summary();
        chains += usize::from(result.chains);
        score = score.wrapping_add(result.score);
        black_box(&board);
    }
    let elapsed = started.elapsed();
    println!(
        "board_bytes={} board_alignment={} copy_elapsed={copy_elapsed:.3?} copy_ns={:.2}",
        std::mem::size_of::<Board>(),
        std::mem::align_of::<Board>(),
        copy_elapsed.as_secs_f64() * 1e9 / iterations as f64,
    );
    println!(
        "iterations={iterations} elapsed={elapsed:.3?} simulation_ns={:.2} simulations_per_second={:.0} chains={} checksum={score}",
        elapsed.as_secs_f64() * 1e9 / iterations as f64,
        iterations as f64 / elapsed.as_secs_f64(),
        chains
    );

    let chain_started = Instant::now();
    let mut chain_checksum = 0u64;
    for index in 0..iterations {
        let mut board = templates[index & 255].clone();
        chain_checksum += u64::from(black_box(&mut board).simulate_chain_count());
        black_box(&board);
    }
    let chain_elapsed = chain_started.elapsed();
    println!(
        "chain_only_elapsed={chain_elapsed:.3?} chain_only_ns={:.2} chain_only_per_second={:.0} checksum={chain_checksum}",
        chain_elapsed.as_secs_f64() * 1e9 / iterations as f64,
        iterations as f64 / chain_elapsed.as_secs_f64(),
    );

    let labels_started = Instant::now();
    let mut label_checksum = 0u64;
    for index in 0..iterations {
        let labels = black_box(&templates[index & 255]).chain_labels();
        label_checksum = label_checksum
            .wrapping_add(u64::from(labels.chains))
            .wrapping_add(
                labels
                    .vanish_step
                    .iter()
                    .map(|&value| u64::from(value))
                    .sum(),
            );
        black_box(labels);
    }
    let labels_elapsed = labels_started.elapsed();
    println!(
        "labels_elapsed={labels_elapsed:.3?} labels_ns={:.2} labels_per_second={:.0} checksum={label_checksum}",
        labels_elapsed.as_secs_f64() * 1e9 / iterations as f64,
        iterations as f64 / labels_elapsed.as_secs_f64(),
    );

    let legal_iterations = iterations.min(100_000);
    let legal_started = Instant::now();
    let mut legal_checksum = 0usize;
    for index in 0..legal_iterations {
        legal_checksum += black_box(legal_placements(&templates[index & 255])).len();
    }
    let legal_elapsed = legal_started.elapsed();
    println!(
        "legal_iterations={legal_iterations} legal_elapsed={legal_elapsed:.3?} legal_ns={:.2} legal_per_second={:.0} checksum={legal_checksum}",
        legal_elapsed.as_secs_f64() * 1e9 / legal_iterations as f64,
        legal_iterations as f64 / legal_elapsed.as_secs_f64(),
    );

    let compact_legal_started = Instant::now();
    let mut compact_legal_checksum = 0usize;
    for index in 0..iterations {
        compact_legal_checksum +=
            black_box(legal_placements_compact(&templates[index & 255])).len();
    }
    let compact_legal_elapsed = compact_legal_started.elapsed();
    println!(
        "compact_legal_elapsed={compact_legal_elapsed:.3?} compact_legal_ns={:.2} compact_legal_per_second={:.0} checksum={compact_legal_checksum}",
        compact_legal_elapsed.as_secs_f64() * 1e9 / iterations as f64,
        iterations as f64 / compact_legal_elapsed.as_secs_f64(),
    );
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
