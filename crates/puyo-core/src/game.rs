use std::collections::VecDeque;

use crate::{Board, CELL_COUNT, Cell, ChainLabels, HEIGHT, SimulationSummary, WIDTH};

pub const POSE_HEIGHT: usize = HEIGHT + 1;
pub const POSE_BITS: usize = WIDTH * POSE_HEIGHT;
pub const MAX_LEGAL_ACTIONS: usize = 22;
pub const DEATH_X: usize = 2;
pub const DEATH_Y: usize = 11;

const SPAWN: Pose = Pose {
    x: 2,
    y: 12,
    rotation: Rotation::Up,
};
const ROTATION_KICKS: [(i8, i8); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Colour {
    Red,
    Green,
    Blue,
    Yellow,
}

impl Colour {
    pub const ALL: [Self; 4] = [Self::Red, Self::Green, Self::Blue, Self::Yellow];

    pub const fn cell(self) -> Cell {
        match self {
            Self::Red => Cell::Red,
            Self::Green => Cell::Green,
            Self::Blue => Cell::Blue,
            Self::Yellow => Cell::Yellow,
        }
    }

    const fn from_index(index: u8) -> Self {
        Self::ALL[index as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pair {
    pub axis: Colour,
    pub child: Colour,
}

#[derive(Debug, Clone)]
pub struct Modern128 {
    queue: [Pair; 128],
    cursor: u8,
}

impl Modern128 {
    pub fn new(seed: u32) -> Self {
        let mut random = ModernRandom(seed);
        for _ in 0..5 {
            random.next();
        }

        let mut queues = [[0u8; 256]; 3];
        for (mode, queue) in queues.iter_mut().enumerate() {
            for (index, colour) in queue.iter_mut().enumerate() {
                *colour = (index % (mode + 3)) as u8;
            }
        }
        for queue in &mut queues {
            shuffle_modern(queue, &mut random);
        }
        let [three_colour, four_colour, five_colour] = &mut queues;
        four_colour[..4].copy_from_slice(&three_colour[..4]);
        five_colour[..4].copy_from_slice(&three_colour[..4]);

        let queue = std::array::from_fn(|index| Pair {
            axis: Colour::from_index(queues[1][index * 2]),
            child: Colour::from_index(queues[1][index * 2 + 1]),
        });
        Self { queue, cursor: 0 }
    }

    pub fn visible(&self) -> [Pair; 3] {
        std::array::from_fn(|offset| self.queue[(usize::from(self.cursor) + offset) & 127])
    }

    pub fn advance(&mut self) {
        self.cursor = self.cursor.wrapping_add(1) & 127;
    }

    pub const fn position(&self) -> u8 {
        self.cursor
    }
}

struct ModernRandom(u32);

impl ModernRandom {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x5d58_8b65).wrapping_add(0x0026_9ec3);
        self.0
    }
}

fn shuffle_modern(queue: &mut [u8; 256], random: &mut ModernRandom) {
    for column in 0..15 {
        for _ in 0..8 {
            let left = (random.next() >> 28) as usize + column * 16;
            let right = (random.next() >> 28) as usize + (column + 1) * 16;
            queue.swap(left, right);
        }
    }
    for column in 0..7 {
        for _ in 0..16 {
            let left = (random.next() >> 27) as usize + column * 32;
            let right = (random.next() >> 27) as usize + (column + 1) * 32;
            queue.swap(left, right);
        }
    }
    for column in 0..3 {
        for _ in 0..32 {
            let left = (random.next() >> 26) as usize + column * 64;
            let right = (random.next() >> 26) as usize + (column + 1) * 64;
            queue.swap(left, right);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rotation {
    Right,
    Down,
    Left,
    Up,
}

impl Rotation {
    const ALL: [Self; 4] = [Self::Right, Self::Down, Self::Left, Self::Up];

    const fn offset(self) -> (i8, i8) {
        match self {
            Self::Right => (1, 0),
            Self::Down => (0, -1),
            Self::Left => (-1, 0),
            Self::Up => (0, 1),
        }
    }

    const fn cw(self) -> Self {
        Self::ALL[(self as usize + 3) & 3]
    }

    const fn ccw(self) -> Self {
        Self::ALL[(self as usize + 1) & 3]
    }

    const fn turn(self) -> Self {
        Self::ALL[(self as usize + 2) & 3]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement {
    pub axis_column: u8,
    pub rotation: Rotation,
}

const ALL_PLACEMENTS: [Placement; MAX_LEGAL_ACTIONS] = [
    Placement {
        axis_column: 0,
        rotation: Rotation::Right,
    },
    Placement {
        axis_column: 1,
        rotation: Rotation::Right,
    },
    Placement {
        axis_column: 2,
        rotation: Rotation::Right,
    },
    Placement {
        axis_column: 3,
        rotation: Rotation::Right,
    },
    Placement {
        axis_column: 4,
        rotation: Rotation::Right,
    },
    Placement {
        axis_column: 0,
        rotation: Rotation::Down,
    },
    Placement {
        axis_column: 1,
        rotation: Rotation::Down,
    },
    Placement {
        axis_column: 2,
        rotation: Rotation::Down,
    },
    Placement {
        axis_column: 3,
        rotation: Rotation::Down,
    },
    Placement {
        axis_column: 4,
        rotation: Rotation::Down,
    },
    Placement {
        axis_column: 5,
        rotation: Rotation::Down,
    },
    Placement {
        axis_column: 1,
        rotation: Rotation::Left,
    },
    Placement {
        axis_column: 2,
        rotation: Rotation::Left,
    },
    Placement {
        axis_column: 3,
        rotation: Rotation::Left,
    },
    Placement {
        axis_column: 4,
        rotation: Rotation::Left,
    },
    Placement {
        axis_column: 5,
        rotation: Rotation::Left,
    },
    Placement {
        axis_column: 0,
        rotation: Rotation::Up,
    },
    Placement {
        axis_column: 1,
        rotation: Rotation::Up,
    },
    Placement {
        axis_column: 2,
        rotation: Rotation::Up,
    },
    Placement {
        axis_column: 3,
        rotation: Rotation::Up,
    },
    Placement {
        axis_column: 4,
        rotation: Rotation::Up,
    },
    Placement {
        axis_column: 5,
        rotation: Rotation::Up,
    },
];

/// Allocation-free legal placement list used while expanding MCTS nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegalPlacements {
    entries: [Placement; MAX_LEGAL_ACTIONS],
    len: u8,
}

impl LegalPlacements {
    pub const fn empty() -> Self {
        Self {
            entries: [ALL_PLACEMENTS[0]; MAX_LEGAL_ACTIONS],
            len: 0,
        }
    }

    pub fn as_slice(&self) -> &[Placement] {
        &self.entries[..usize::from(self.len)]
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Left,
    Right,
    Down,
    RotateCw,
    RotateCcw,
    Rotate180,
    HardDrop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalAction {
    pub placement: Placement,
    pub route: Vec<Input>,
    pub route_frames: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pose {
    x: i8,
    y: i8,
    rotation: Rotation,
}

impl Pose {
    fn bit(self) -> u128 {
        1u128 << (self.y as usize * WIDTH + self.x as usize)
    }

    fn index(self) -> usize {
        self.rotation as usize * POSE_BITS + self.y as usize * WIDTH + self.x as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachablePoses(pub [u128; 4]);

impl ReachablePoses {
    pub fn contains(self, x: usize, y: usize, rotation: Rotation) -> bool {
        self.0[rotation as usize] & (1u128 << (y * WIDTH + x)) != 0
    }
}

pub fn reachable_poses(board: &Board) -> ReachablePoses {
    reachable_poses_with_heights(&board.column_heights())
}

fn reachable_poses_with_heights(heights: &[u8; WIDTH]) -> ReachablePoses {
    let valid = valid_pose_masks(heights);
    if valid[Rotation::Up as usize] & SPAWN.bit() == 0 {
        return ReachablePoses([0; 4]);
    }
    let mut reached = [0u128; 4];
    reached[Rotation::Up as usize] = SPAWN.bit();
    let mut frontier = reached;
    let x0_mask = repeated_pose_column(0);
    let x5_mask = repeated_pose_column(WIDTH - 1);
    let bottom_mask = (1u128 << WIDTH) - 1;

    while frontier.iter().any(|&bits| bits != 0) {
        let mut next = [0u128; 4];
        for rotation in Rotation::ALL {
            let slot = rotation as usize;
            let source = frontier[slot];
            next[slot] |= ((source & !x0_mask) >> 1) & valid[slot];
            next[slot] |= ((source & !x5_mask) << 1) & valid[slot];
            next[slot] |= ((source & !bottom_mask) >> WIDTH) & valid[slot];
        }
        for rotation in Rotation::ALL {
            let source = frontier[rotation as usize];
            add_quarter_turn(source, rotation.cw(), &valid, &mut next);
            add_quarter_turn(source, rotation.ccw(), &valid, &mut next);
            let target = rotation.turn();
            let dy = match target {
                Rotation::Up => -1,
                Rotation::Down => 1,
                _ => 0,
            };
            next[target as usize] |= shift_pose_bits(source, 0, dy) & valid[target as usize];
        }
        for rotation in Rotation::ALL {
            let slot = rotation as usize;
            next[slot] &= !reached[slot];
            reached[slot] |= next[slot];
        }
        frontier = next;
    }
    ReachablePoses(reached)
}

fn add_quarter_turn(source: u128, target: Rotation, valid: &[u128; 4], next: &mut [u128; 4]) {
    let target_valid = valid[target as usize];
    let direct = source & target_valid;
    let blocked = source & !target_valid;
    let (dx, dy) = ROTATION_KICKS[target as usize];
    next[target as usize] |= direct | (shift_pose_bits(blocked, dx, dy) & target_valid);
}

fn shift_pose_bits(bits: u128, dx: i8, dy: i8) -> u128 {
    let horizontally = match dx {
        -1 => (bits & !repeated_pose_column(0)) >> 1,
        0 => bits,
        1 => (bits & !repeated_pose_column(WIDTH - 1)) << 1,
        _ => unreachable!(),
    };
    match dy {
        -1 => horizontally >> WIDTH,
        0 => horizontally,
        1 => horizontally << WIDTH,
        _ => unreachable!(),
    }
}

const fn repeated_pose_column(column: usize) -> u128 {
    let mut result = 0u128;
    let mut y = 0;
    while y < POSE_HEIGHT {
        result |= 1u128 << (y * WIDTH + column);
        y += 1;
    }
    result
}

fn valid_pose_masks(heights: &[u8; WIDTH]) -> [u128; 4] {
    let mut free = 0u128;
    for (x, &height) in heights.iter().enumerate() {
        for y in usize::from(height)..POSE_HEIGHT {
            free |= 1u128 << (y * WIDTH + x);
        }
    }
    let x0 = repeated_pose_column(0);
    let x5 = repeated_pose_column(WIDTH - 1);
    [
        free & (free >> 1) & !x5,
        free & (free << WIDTH),
        free & (free << 1) & !x0,
        free & (free >> WIDTH),
    ]
}

fn valid_pose(board: &Board, pose: Pose) -> bool {
    let (dx, dy) = pose.rotation.offset();
    valid_falling_cell(board, pose.x, pose.y) && valid_falling_cell(board, pose.x + dx, pose.y + dy)
}

fn valid_falling_cell(board: &Board, x: i8, y: i8) -> bool {
    x >= 0
        && x < WIDTH as i8
        && y >= 0
        && y < POSE_HEIGHT as i8
        && (y == HEIGHT as i8 || board.get(x as usize, y as usize) == Cell::Empty)
}

fn transition(board: &Board, pose: Pose, input: Input) -> Option<Pose> {
    match input {
        Input::Left | Input::Right | Input::Down => {
            let (dx, dy) = match input {
                Input::Left => (-1, 0),
                Input::Right => (1, 0),
                Input::Down => (0, -1),
                _ => unreachable!(),
            };
            let next = Pose {
                x: pose.x + dx,
                y: pose.y + dy,
                ..pose
            };
            valid_pose(board, next).then_some(next)
        }
        Input::RotateCw | Input::RotateCcw => {
            let rotation = if input == Input::RotateCw {
                pose.rotation.cw()
            } else {
                pose.rotation.ccw()
            };
            let direct = Pose { rotation, ..pose };
            if valid_pose(board, direct) {
                return Some(direct);
            }
            let (dx, dy) = ROTATION_KICKS[rotation as usize];
            let kicked = Pose {
                x: pose.x + dx,
                y: pose.y + dy,
                rotation,
            };
            valid_pose(board, kicked).then_some(kicked)
        }
        Input::Rotate180 => {
            let rotation = pose.rotation.turn();
            let dy = match rotation {
                Rotation::Up => -1,
                Rotation::Down => 1,
                _ => 0,
            };
            let next = Pose {
                y: pose.y + dy,
                rotation,
                ..pose
            };
            valid_pose(board, next).then_some(next)
        }
        Input::HardDrop => None,
    }
}

pub fn legal_actions(board: &Board) -> Vec<LegalAction> {
    legal_placements_compact(board)
        .as_slice()
        .iter()
        .copied()
        .filter_map(|placement| {
            shortest_route(board, placement).map(|route| LegalAction {
                route_frames: route_frame_cost(&route),
                route,
                placement,
            })
        })
        .collect()
}

/// Allocation is bounded to at most 22 compact placements. This is the MCTS
/// hot-path API; route materialisation belongs at the played root only.
pub fn legal_placements(board: &Board) -> Vec<Placement> {
    legal_placements_compact(board).as_slice().to_vec()
}

/// Directly derives the legal actions from the six column heights.
///
/// With gravity-settled columns there are no cavities to tuck into. Once the
/// spawn cell is free, a pair can turn horizontal on row 13, cross even a
/// completely full column, and rotate/drop into every geometrically fitting
/// destination. Consequently only heights 12 and 13 affect legality.
#[inline]
pub fn legal_placements_compact(board: &Board) -> LegalPlacements {
    let heights = board.column_heights();
    let mut mask = legal_placement_mask(&heights);
    let mut result = LegalPlacements::empty();
    while mask != 0 {
        let index = mask.trailing_zeros() as usize;
        result.entries[usize::from(result.len)] = ALL_PLACEMENTS[index];
        result.len += 1;
        mask &= mask - 1;
    }
    result
}

#[inline]
fn legal_placement_mask(heights: &[u8; WIDTH]) -> u32 {
    if heights[SPAWN.x as usize] >= HEIGHT as u8 {
        return 0;
    }

    let mut open = 0u32;
    let mut vertical = 0u32;
    for (x, &height) in heights.iter().enumerate() {
        open |= u32::from(height < HEIGHT as u8) << x;
        vertical |= u32::from(height < (HEIGHT - 1) as u8) << x;
    }

    let right = open & (open >> 1) & 0x1f;
    let down = vertical << 5;
    let left = (open & (open << 1) & 0x3e) << 10;
    let up = vertical << 16;
    right | down | left | up
}

#[inline]
pub fn is_legal_placement(board: &Board, placement: Placement) -> bool {
    let x = u32::from(placement.axis_column);
    let index = match placement.rotation {
        Rotation::Right if x < 5 => x,
        Rotation::Down if x < 6 => 5 + x,
        Rotation::Left if (1..6).contains(&x) => 10 + x,
        Rotation::Up if x < 6 => 16 + x,
        _ => return false,
    };
    if index >= MAX_LEGAL_ACTIONS as u32 {
        return false;
    }
    legal_placement_mask(&board.column_heights()) & (1 << index) != 0
}

pub fn route_to_placement(board: &Board, placement: Placement) -> Option<LegalAction> {
    if !is_legal_placement(board, placement) {
        return None;
    }
    shortest_route(board, placement).map(|route| LegalAction {
        route_frames: route_frame_cost(&route),
        route,
        placement,
    })
}

fn route_frame_cost(route: &[Input]) -> u16 {
    route
        .iter()
        .map(|input| u16::from(*input != Input::HardDrop) * 2)
        .sum()
}

fn shortest_route(board: &Board, target: Placement) -> Option<Vec<Input>> {
    if !valid_pose(board, SPAWN) {
        return None;
    }
    const STATES: usize = POSE_BITS * 4;
    const NONE: u16 = u16::MAX;
    let mut parent = [NONE; STATES];
    let mut parent_input = [Input::HardDrop; STATES];
    let mut queue = VecDeque::from([SPAWN]);
    parent[SPAWN.index()] = SPAWN.index() as u16;

    while let Some(pose) = queue.pop_front() {
        if pose.x == target.axis_column as i8 && pose.rotation == target.rotation {
            let mut route = Vec::new();
            let mut index = pose.index();
            while parent[index] as usize != index {
                route.push(parent_input[index]);
                index = parent[index] as usize;
            }
            route.reverse();
            route.push(Input::HardDrop);
            return Some(route);
        }
        for input in [
            Input::Left,
            Input::Right,
            Input::Down,
            Input::RotateCw,
            Input::RotateCcw,
            Input::Rotate180,
        ] {
            let Some(next) = transition(board, pose, input) else {
                continue;
            };
            let index = next.index();
            if parent[index] != NONE {
                continue;
            }
            parent[index] = pose.index() as u16;
            parent_input[index] = input;
            queue.push_back(next);
        }
    }
    None
}

fn landing_cells(board: &Board, placement: Placement) -> Option<[(usize, usize); 2]> {
    landing_cells_with_heights(&board.column_heights(), placement)
}

fn landing_cells_with_heights(
    heights: &[u8; WIDTH],
    placement: Placement,
) -> Option<[(usize, usize); 2]> {
    let x = usize::from(placement.axis_column);
    match placement.rotation {
        Rotation::Right | Rotation::Left => {
            let child_x = if placement.rotation == Rotation::Right {
                x.checked_add(1)?
            } else {
                x.checked_sub(1)?
            };
            if child_x >= WIDTH {
                return None;
            }
            let axis_y = usize::from(heights[x]);
            let child_y = usize::from(heights[child_x]);
            (axis_y < HEIGHT && child_y < HEIGHT).then_some([(x, axis_y), (child_x, child_y)])
        }
        Rotation::Up | Rotation::Down => {
            let bottom = usize::from(heights[x]);
            if bottom + 1 >= HEIGHT {
                return None;
            }
            if placement.rotation == Rotation::Up {
                Some([(x, bottom), (x, bottom + 1)])
            } else {
                Some([(x, bottom + 1), (x, bottom)])
            }
        }
    }
}

pub fn place_pair(board: &mut Board, pair: Pair, placement: Placement) -> Option<u8> {
    let [axis, child] = landing_cells(board, placement)?;
    board.set(axis.0, axis.1, pair.axis.cell());
    board.set(child.0, child.1, pair.child.cell());
    Some(axis.1.abs_diff(child.1) as u8)
}

#[derive(Debug, Clone)]
pub struct GameState {
    board: Board,
    generator: Modern128,
    placements: u32,
    maximum_chain: u8,
    dead: bool,
    observed_colour_counts: [u16; 4],
}

#[derive(Debug, Clone)]
pub struct PublicState {
    pub board: Board,
    pub pieces: [Pair; 3],
    pub queue_position: u8,
    pub estimated_unseen_colour_counts: [u16; 4],
    pub placements: u32,
    pub maximum_chain: u8,
    pub dead: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub simulation: SimulationSummary,
    pub labels: ChainLabels,
    pub split_distance: u8,
    pub dead: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FastStepOutcome {
    pub simulation: SimulationSummary,
    pub split_distance: u8,
    pub dead: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    Terminal,
    IllegalPlacement,
}

impl GameState {
    pub fn new(seed: u32) -> Self {
        let generator = Modern128::new(seed);
        Self {
            board: Board::empty(),
            observed_colour_counts: colour_counts(&generator.visible()),
            generator,
            placements: 0,
            maximum_chain: 0,
            dead: false,
        }
    }

    pub fn from_board(seed: u32, board: Board) -> Self {
        let dead = board.get(DEATH_X, DEATH_Y) != Cell::Empty;
        let generator = Modern128::new(seed);
        Self {
            board,
            observed_colour_counts: colour_counts(&generator.visible()),
            generator,
            placements: 0,
            maximum_chain: 0,
            dead,
        }
    }

    pub fn resume(seed: u32, board: Board, placements: u32, maximum_chain: u8) -> Self {
        let mut state = Self::from_board(seed, board);
        for _ in 0..(placements & 127) {
            state.generator.advance();
            let revealed = state.generator.visible()[2];
            state.observed_colour_counts[revealed.axis as usize] += 1;
            state.observed_colour_counts[revealed.child as usize] += 1;
        }
        state.placements = placements;
        state.maximum_chain = maximum_chain;
        state
    }

    pub fn public_state(&self) -> PublicState {
        PublicState {
            board: self.board.clone(),
            pieces: self.generator.visible(),
            queue_position: self.generator.position(),
            estimated_unseen_colour_counts: std::array::from_fn(|colour| {
                64u16.saturating_sub(self.observed_colour_counts[colour])
            }),
            placements: self.placements,
            maximum_chain: self.maximum_chain,
            dead: self.dead,
        }
    }

    pub fn legal_actions(&self) -> Vec<LegalAction> {
        if self.dead {
            Vec::new()
        } else {
            legal_actions(&self.board)
        }
    }

    pub fn legal_placements(&self) -> Vec<Placement> {
        if self.dead {
            Vec::new()
        } else {
            legal_placements(&self.board)
        }
    }

    pub fn step(&mut self, placement: Placement) -> Result<StepOutcome, StepError> {
        let split_distance = self.place_current(placement)?;
        let labels = self.board.chain_labels();
        let fast = self.finish_step(split_distance);
        Ok(StepOutcome {
            simulation: fast.simulation,
            labels,
            split_distance,
            dead: fast.dead,
        })
    }

    /// MCTS hot path. Dense auxiliary labels are generated only for the move
    /// actually played into the self-play trajectory, not for search branches.
    pub fn step_fast(&mut self, placement: Placement) -> Result<FastStepOutcome, StepError> {
        let split_distance = self.place_current(placement)?;
        Ok(self.finish_step(split_distance))
    }

    fn place_current(&mut self, placement: Placement) -> Result<u8, StepError> {
        if self.dead {
            return Err(StepError::Terminal);
        }
        if !is_legal_placement(&self.board, placement) {
            return Err(StepError::IllegalPlacement);
        }
        let pair = self.generator.visible()[0];
        place_pair(&mut self.board, pair, placement).ok_or(StepError::IllegalPlacement)
    }

    fn finish_step(&mut self, split_distance: u8) -> FastStepOutcome {
        let simulation = self.board.simulate_summary();
        self.maximum_chain = self.maximum_chain.max(simulation.chains);
        self.placements += 1;
        self.generator.advance();
        let revealed = self.generator.visible()[2];
        self.observed_colour_counts[revealed.axis as usize] += 1;
        self.observed_colour_counts[revealed.child as usize] += 1;
        self.dead =
            self.board.get(DEATH_X, DEATH_Y) != Cell::Empty || !valid_pose(&self.board, SPAWN);
        FastStepOutcome {
            simulation,
            split_distance,
            dead: self.dead,
        }
    }
}

fn colour_counts(pairs: &[Pair]) -> [u16; 4] {
    let mut result = [0u16; 4];
    for pair in pairs {
        result[pair.axis as usize] += 1;
        result[pair.child as usize] += 1;
    }
    result
}

const _: () = assert!(CELL_COUNT == 78);

#[cfg(test)]
mod tests {
    use super::*;

    fn colour_ids(pairs: [Pair; 3]) -> [u8; 6] {
        [
            pairs[0].axis as u8,
            pairs[0].child as u8,
            pairs[1].axis as u8,
            pairs[1].child as u8,
            pairs[2].axis as u8,
            pairs[2].child as u8,
        ]
    }

    #[test]
    fn modern_queue_is_deterministic_balanced_and_initially_three_colour() {
        for seed in [0, 1, 0x1234_5678, u32::MAX] {
            let mut left = Modern128::new(seed);
            let mut right = Modern128::new(seed);
            let initial = colour_ids(left.visible());
            assert!(initial[..4].iter().copied().max().unwrap() <= 2);
            let mut counts = [0u16; 4];
            for _ in 0..128 {
                assert_eq!(left.visible(), right.visible());
                let pair = left.visible()[0];
                counts[pair.axis as usize] += 1;
                counts[pair.child as usize] += 1;
                left.advance();
                right.advance();
            }
            // Four overwritten entries can perturb the otherwise exact 64 count.
            assert!(counts.iter().all(|&count| (60..=68).contains(&count)));
            assert_eq!(left.position(), 0);
        }
    }

    #[test]
    fn modern_queue_matches_independent_cpp_golden_prefixes() {
        let cases = [
            (0, "00202323011121120003113221330223"),
            (1, "01211123310322221121033310331223"),
            (0x1234_5678, "21221133311031211223323330132223"),
            (u32::MAX, "12220123012330211103111121223230"),
        ];
        for (seed, expected) in cases {
            let generator = Modern128::new(seed);
            let actual: String = generator.queue[..16]
                .iter()
                .flat_map(|pair| [pair.axis as u8, pair.child as u8])
                .map(|value| char::from(b'0' + value))
                .collect();
            assert_eq!(actual, expected, "seed={seed:#010x}");
        }
    }

    #[test]
    fn empty_board_has_all_twenty_two_placements() {
        let actions = legal_actions(&Board::empty());
        assert_eq!(actions.len(), MAX_LEGAL_ACTIONS);
        assert!(
            actions
                .iter()
                .all(|action| action.route.last() == Some(&Input::HardDrop))
        );
    }

    #[test]
    fn direct_legality_matches_reachability_for_every_top_profile() {
        // Heights below 12 are equivalent for placement legality. Exhaust all
        // 3^6 combinations of low, row-12 occupied, and completely full.
        for encoded in 0..3usize.pow(WIDTH as u32) {
            let mut value = encoded;
            let mut board = Board::empty();
            for x in 0..WIDTH {
                let height = [0, HEIGHT - 1, HEIGHT][value % 3];
                value /= 3;
                for y in 0..height {
                    board.set(x, y, Colour::Red.cell());
                }
            }
            assert_eq!(
                legal_placements(&board),
                reference_legal_placements(&board),
                "top profile={encoded}"
            );
        }
    }

    #[test]
    fn one_hundred_thousand_direct_legality_results_match_reachability() {
        let mut random = 0xa409_3822_299f_31d0u64;
        for case in 0..100_000 {
            let mut board = Board::empty();
            for x in 0..WIDTH {
                let height = (next_random(&mut random) % (HEIGHT as u64 + 1)) as usize;
                for y in 0..height {
                    board.set(x, y, Colour::Red.cell());
                }
            }
            assert_eq!(
                legal_placements(&board),
                reference_legal_placements(&board),
                "case={case} heights={:?}",
                board.column_heights()
            );
        }
    }

    fn reference_legal_placements(board: &Board) -> Vec<Placement> {
        let heights = board.column_heights();
        let reached = reachable_poses_with_heights(&heights);
        ALL_PLACEMENTS
            .iter()
            .copied()
            .filter(|&placement| {
                landing_cells_with_heights(&heights, placement).is_some()
                    && (0..POSE_HEIGHT).any(|y| {
                        reached.contains(usize::from(placement.axis_column), y, placement.rotation)
                    })
            })
            .collect()
    }

    #[test]
    fn bit_reachability_matches_scalar_bfs_on_adversarial_boards() {
        let mut random = 0x243f_6a88_85a3_08d3u64;
        for case in 0..10_000 {
            let mut board = Board::empty();
            for x in 0..WIDTH {
                let height = (next_random(&mut random) % 13) as usize;
                for y in 0..height {
                    board.set(
                        x,
                        y,
                        Colour::ALL[(next_random(&mut random) & 3) as usize].cell(),
                    );
                }
            }
            let fast = reachable_poses(&board);
            let slow = scalar_reachable(&board);
            assert_eq!(fast.0, slow.0, "case={case}");
        }
    }

    #[test]
    fn every_materialised_route_replays_to_its_placement() {
        let mut random = 0x1319_8a2e_0370_7344u64;
        for case in 0..1_000 {
            let mut board = Board::empty();
            for x in 0..WIDTH {
                let height = (next_random(&mut random) % 11) as usize;
                for y in 0..height {
                    board.set(
                        x,
                        y,
                        Colour::ALL[(next_random(&mut random) & 3) as usize].cell(),
                    );
                }
            }
            for action in legal_actions(&board) {
                let mut pose = SPAWN;
                for &input in &action.route[..action.route.len() - 1] {
                    pose = transition(&board, pose, input)
                        .unwrap_or_else(|| panic!("invalid route case={case} input={input:?}"));
                }
                assert_eq!(action.route.last(), Some(&Input::HardDrop));
                assert_eq!(pose.x, action.placement.axis_column as i8, "case={case}");
                assert_eq!(pose.rotation, action.placement.rotation, "case={case}");
                assert!(landing_cells(&board, action.placement).is_some());
            }
        }
    }

    fn scalar_reachable(board: &Board) -> ReachablePoses {
        if !valid_pose(board, SPAWN) {
            return ReachablePoses([0; 4]);
        }
        let mut result = [0u128; 4];
        result[Rotation::Up as usize] = SPAWN.bit();
        let mut queue = VecDeque::from([SPAWN]);
        while let Some(pose) = queue.pop_front() {
            for input in [
                Input::Left,
                Input::Right,
                Input::Down,
                Input::RotateCw,
                Input::RotateCcw,
                Input::Rotate180,
            ] {
                let Some(next) = transition(board, pose, input) else {
                    continue;
                };
                let slot = &mut result[next.rotation as usize];
                if *slot & next.bit() != 0 {
                    continue;
                }
                *slot |= next.bit();
                queue.push_back(next);
            }
        }
        ReachablePoses(result)
    }

    #[test]
    fn game_step_advances_only_public_window_and_preserves_gravity() {
        let mut game = GameState::new(42);
        let before = game.public_state();
        let action = game.legal_actions()[0].placement;
        let outcome = game.step(action).unwrap();
        let after = game.public_state();
        assert_eq!(before.pieces[1], after.pieces[0]);
        assert_eq!(before.pieces[2], after.pieces[1]);
        assert_eq!(after.queue_position, 1);
        assert_eq!(after.placements, 1);
        assert!(after.board.validate_gravity().is_ok());
        assert_eq!(outcome.simulation.chains, outcome.labels.chains);
    }

    #[test]
    fn fast_step_matches_labelled_step() {
        let mut labelled = GameState::new(0xdead_beef);
        let mut fast = labelled.clone();
        for _ in 0..32 {
            let Some(placement) = labelled.legal_placements().first().copied() else {
                break;
            };
            let with_labels = labelled.step(placement).unwrap();
            let without_labels = fast.step_fast(placement).unwrap();
            assert_eq!(with_labels.simulation, without_labels.simulation);
            assert_eq!(with_labels.split_distance, without_labels.split_distance);
            assert_eq!(with_labels.dead, without_labels.dead);
            assert_eq!(labelled.public_state().board, fast.public_state().board);
            if with_labels.dead {
                break;
            }
        }
    }

    fn next_random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }
}
