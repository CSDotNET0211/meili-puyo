//! Interactive CUI play mode.
//!
//! Controls:
//!   ←/→ or h/l  move pair horizontally
//!   ↓    or j   soft drop
//!   z / x       rotate CCW / CW
//!   c           rotate 180
//!   space       hard drop (locks the pair)
//!   q           quit
//!
//! If SIMULATIONS > 0 the AI suggestion for the current position is shown.

use std::io::{self, Write};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType};

use puyo_ai::mcts::{SearchConfig, SearchState, search};
use puyo_core::game::{GameState, Pair, Placement, Rotation, is_legal_placement};
use puyo_core::{Board, Cell, HEIGHT, WIDTH};

/// Falling pair state. `x`,`y` is the axis cell; rotation gives the child offset.
#[derive(Debug, Clone, Copy)]
struct Falling {
    x: i8,
    y: i8,
    rotation: Rotation,
}

impl Falling {
    fn spawn() -> Self {
        Self {
            x: 2,
            y: HEIGHT as i8, // hidden row 13
            rotation: Rotation::Up,
        }
    }

    fn cells(self) -> [(i8, i8); 2] {
        let (dx, dy) = match self.rotation {
            Rotation::Right => (1, 0),
            Rotation::Down => (0, -1),
            Rotation::Left => (-1, 0),
            Rotation::Up => (0, 1),
        };
        [(self.x, self.y), (self.x + dx, self.y + dy)]
    }

    fn placement(self) -> Placement {
        Placement {
            axis_column: self.x as u8,
            rotation: self.rotation,
        }
    }
}

fn valid(board: &Board, x: i8, y: i8) -> bool {
    x >= 0
        && x < WIDTH as i8
        && y >= 0
        && y <= HEIGHT as i8
        && (y == HEIGHT as i8 || board.get(x as usize, y as usize) == Cell::Empty)
}

fn valid_pose(board: &Board, pose: Falling) -> bool {
    pose.cells().iter().all(|&(x, y)| valid(board, x, y))
}

const KICKS: [(i8, i8); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];

fn try_rotate(board: &Board, pose: Falling, target: Rotation) -> Option<Falling> {
    let direct = Falling {
        rotation: target,
        ..pose
    };
    if valid_pose(board, direct) {
        return Some(direct);
    }
    let (dx, dy) = KICKS[target as usize];
    let kicked = Falling {
        x: pose.x + dx,
        y: pose.y + dy,
        rotation: target,
    };
    valid_pose(board, kicked).then_some(kicked)
}

fn rotate_cw(r: Rotation) -> Rotation {
    match r {
        Rotation::Up => Rotation::Right,
        Rotation::Right => Rotation::Down,
        Rotation::Down => Rotation::Left,
        Rotation::Left => Rotation::Up,
    }
}

fn rotate_ccw(r: Rotation) -> Rotation {
    match r {
        Rotation::Up => Rotation::Left,
        Rotation::Left => Rotation::Down,
        Rotation::Down => Rotation::Right,
        Rotation::Right => Rotation::Up,
    }
}

fn rotate_180(r: Rotation) -> Rotation {
    match r {
        Rotation::Up => Rotation::Down,
        Rotation::Down => Rotation::Up,
        Rotation::Left => Rotation::Right,
        Rotation::Right => Rotation::Left,
    }
}

fn cell_color(cell: Cell) -> Color {
    match cell {
        Cell::Empty => Color::Black,
        Cell::Garbage => Color::DarkGrey,
        Cell::Red => Color::Red,
        Cell::Green => Color::Green,
        Cell::Blue => Color::Blue,
        Cell::Yellow => Color::Yellow,
    }
}

fn pair_color(pair_cell: puyo_core::game::Colour) -> Color {
    match pair_cell {
        puyo_core::game::Colour::Red => Color::Red,
        puyo_core::game::Colour::Green => Color::Green,
        puyo_core::game::Colour::Blue => Color::Blue,
        puyo_core::game::Colour::Yellow => Color::Yellow,
    }
}

fn draw(
    stdout: &mut io::Stdout,
    game: &GameState,
    falling: Option<(Falling, Pair)>,
    hint: &str,
) -> io::Result<()> {
    let public = game.public_state();
    queue!(stdout, cursor::MoveTo(0, 0), Clear(ClearType::All))?;

    // Board: rows 12..0 top-to-bottom; row 12 (index 12) is the hidden 13th row.
    for y in (0..HEIGHT).rev() {
        let hidden = y == HEIGHT - 1;
        queue!(
            stdout,
            SetForegroundColor(Color::DarkGrey),
            Print(if hidden { ">" } else { " " }),
        )?;
        for x in 0..WIDTH {
            let mut cell = public.board.get(x, y);
            if let Some((pose, pair)) = falling {
                let [axis, child] = pose.cells();
                if (x as i8, y as i8) == axis {
                    cell = match pair.axis {
                        puyo_core::game::Colour::Red => Cell::Red,
                        puyo_core::game::Colour::Green => Cell::Green,
                        puyo_core::game::Colour::Blue => Cell::Blue,
                        puyo_core::game::Colour::Yellow => Cell::Yellow,
                    };
                } else if (x as i8, y as i8) == child {
                    cell = match pair.child {
                        puyo_core::game::Colour::Red => Cell::Red,
                        puyo_core::game::Colour::Green => Cell::Green,
                        puyo_core::game::Colour::Blue => Cell::Blue,
                        puyo_core::game::Colour::Yellow => Cell::Yellow,
                    };
                }
            }
            queue!(
                stdout,
                SetBackgroundColor(cell_color(cell)),
                Print("  "),
                ResetColor
            )?;
        }
        queue!(stdout, Print("\n"))?;
    }
    queue!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print("  ------------\n"),
        ResetColor
    )?;

    // Next pieces
    queue!(stdout, Print(" next: "))?;
    for (i, pair) in public.pieces.iter().take(2).enumerate() {
        if i > 0 {
            queue!(stdout, Print("   "))?;
        }
        queue!(
            stdout,
            SetBackgroundColor(pair_color(pair.axis)),
            Print("  "),
            ResetColor,
            SetBackgroundColor(pair_color(pair.child)),
            Print("  "),
            ResetColor
        )?;
    }
    queue!(
        stdout,
        Print(format!(
            "\n\n placements={} max_chain={} dead={}\n",
            public.placements, public.maximum_chain, public.dead
        ))
    )?;
    if !hint.is_empty() {
        queue!(stdout, Print(format!(" {hint}\n")))?;
    }
    queue!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(" ←→/hl move  ↓/j drop  z/x rotate  c 180  space hard-drop  q quit\n"),
        ResetColor
    )?;
    stdout.flush()
}

fn ai_hint(game: &GameState, simulations: u32) -> String {
    if simulations == 0 {
        return String::new();
    }
    let public = game.public_state();
    if public.dead {
        return String::new();
    }
    let result = search(
        SearchState::from(&public),
        SearchConfig {
            simulations,
            ..SearchConfig::default()
        },
    );
    match result.best() {
        Some(best) => format!(
            "AI: column {} rotation {:?} (visits {} value {:.3})",
            best.placement.axis_column, best.placement.rotation, best.visits, best.mean_value
        ),
        None => "AI: no moves".to_owned(),
    }
}

pub fn run(seed: u32, simulations: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut game = GameState::new(seed);
    let mut falling = Falling::spawn();
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    let result = play_loop(&mut stdout, &mut game, &mut falling, simulations);
    terminal::disable_raw_mode()?;
    queue!(stdout, cursor::MoveTo(0, 30), Clear(ClearType::FromCursorDown))?;
    stdout.flush()?;
    result
}

fn play_loop(
    stdout: &mut io::Stdout,
    game: &mut GameState,
    falling: &mut Falling,
    simulations: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let public = game.public_state();
        if public.dead {
            draw(stdout, game, None, "GAME OVER — press q")?;
        } else {
            let pair = public.pieces[0];
            let hint = ai_hint(game, simulations);
            draw(stdout, game, Some((*falling, pair)), &hint)?;
        }

        let Event::Key(KeyEvent {
            code,
            modifiers,
            kind,
            ..
        }) = event::read()?
        else {
            continue;
        };
        if kind == KeyEventKind::Release {
            continue;
        }
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            return Ok(());
        }

        let public = game.public_state();
        if public.dead {
            if code == KeyCode::Char('q') {
                return Ok(());
            }
            continue;
        }

        let board = &public.board;
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Left | KeyCode::Char('h') => {
                let next = Falling {
                    x: falling.x - 1,
                    ..*falling
                };
                if valid_pose(board, next) {
                    *falling = next;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let next = Falling {
                    x: falling.x + 1,
                    ..*falling
                };
                if valid_pose(board, next) {
                    *falling = next;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = Falling {
                    y: falling.y - 1,
                    ..*falling
                };
                if valid_pose(board, next) {
                    *falling = next;
                }
            }
            KeyCode::Char('x') | KeyCode::Up => {
                if let Some(next) = try_rotate(board, *falling, rotate_cw(falling.rotation)) {
                    *falling = next;
                }
            }
            KeyCode::Char('z') => {
                if let Some(next) = try_rotate(board, *falling, rotate_ccw(falling.rotation)) {
                    *falling = next;
                }
            }
            KeyCode::Char('c') => {
                if let Some(next) = try_rotate(board, *falling, rotate_180(falling.rotation)) {
                    *falling = next;
                }
            }
            KeyCode::Char(' ') => {
                // Hard drop: sink until blocked.
                while valid_pose(
                    board,
                    Falling {
                        y: falling.y - 1,
                        ..*falling
                    },
                ) {
                    falling.y -= 1;
                }
                let placement = falling.placement();
                if !is_legal_placement(board, placement) {
                    // Pair is above the field or otherwise not a legal final
                    // placement; treat as death move anyway.
                }
                match game.step_fast(placement) {
                    Ok(outcome) => {
                        if outcome.dead {
                            draw(stdout, game, None, "GAME OVER — press q")?;
                        }
                        *falling = Falling::spawn();
                    }
                    Err(error) => {
                        draw(stdout, game, None, &format!("step error: {error:?} — press q"))?;
                    }
                }
            }
            _ => {}
        }
    }
}
