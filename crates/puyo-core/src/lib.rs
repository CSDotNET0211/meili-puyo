//! Fast SIMD bitboard Puyo Puyo rules.
//!
//! The bit layout follows chapter 3 of the puyoai report: each x coordinate
//! occupies one 16-bit SIMD lane and three 128-bit bit planes encode a cell.
//! Rows 1 through 12 can vanish; row 13 is the hidden row and can fall.

pub mod board;
pub mod game;

pub use board::{
    Board, CELL_COUNT, Cell, ChainLabels, HEIGHT, ParseBoardError, SimulationResult,
    SimulationSummary, StepResult, VISIBLE_HEIGHT, WIDTH,
};
