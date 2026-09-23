//! Chance-PUCT search and evaluators for puyo-core.

pub mod mcts;

pub use mcts::{
    BaselineEvaluator, Evaluation, Evaluator, RootAction, SearchConfig, SearchResult,
    SearchState, search, search_with_evaluator,
};
