//! Standalone Puyo Puyo chain simulator.
//!
//! The bit layout follows chapter 3 of the supplied puyoai report: each x
//! coordinate occupies one 16-bit SIMD lane and three 128-bit bit planes encode
//! a cell. Rows 1 through 12 can vanish; row 13 is the hidden row and can fall.

#![cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]

#[cfg(not(target_arch = "x86_64"))]
compile_error!("puyo-simulator currently requires x86_64 (SSE2 is baseline on x86_64)");

use std::arch::x86_64::*;
use std::fmt;

pub const WIDTH: usize = 6;
pub const HEIGHT: usize = 13;
pub const VISIBLE_HEIGHT: usize = 12;
pub const CELL_COUNT: usize = WIDTH * HEIGHT;

const PLANE_COUNT: usize = 3;
const INNER_X_OFFSET: usize = 1;
const INNER_Y_OFFSET: usize = 1;
const VISIBLE_COLUMN_MASK: u16 = 0x1ffe;
const BOARD_COLUMN_MASK: u16 = 0x3ffe;

const fn repeated_lane_mask(column: u16) -> u128 {
    let mut result = 0u128;
    let mut x = 0;
    while x < WIDTH {
        result |= (column as u128) << ((x + INNER_X_OFFSET) * 16);
        x += 1;
    }
    result
}

const VISIBLE_MASK: u128 = repeated_lane_mask(VISIBLE_COLUMN_MASK);
const BOARD_MASK: u128 = repeated_lane_mask(BOARD_COLUMN_MASK);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Cell {
    #[default]
    Empty = 0b000,
    Garbage = 0b001,
    Red = 0b100,
    Green = 0b101,
    Blue = 0b110,
    Yellow = 0b111,
}

impl Cell {
    pub const COLORS: [Self; 4] = [Self::Red, Self::Green, Self::Blue, Self::Yellow];

    pub fn from_char(value: char) -> Option<Self> {
        match value {
            '.' | 'E' | 'e' => Some(Self::Empty),
            'O' | 'o' => Some(Self::Garbage),
            'R' | 'r' => Some(Self::Red),
            'G' | 'g' => Some(Self::Green),
            'B' | 'b' => Some(Self::Blue),
            'Y' | 'y' => Some(Self::Yellow),
            _ => None,
        }
    }

    pub const fn as_char(self) -> char {
        match self {
            Self::Empty => '.',
            Self::Garbage => 'O',
            Self::Red => 'R',
            Self::Green => 'G',
            Self::Blue => 'B',
            Self::Yellow => 'Y',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseBoardError {
    WrongRowCount {
        actual: usize,
    },
    WrongColumnCount {
        row: usize,
        actual: usize,
    },
    InvalidCell {
        row: usize,
        column: usize,
        value: char,
    },
    FloatingCell {
        row: usize,
        column: usize,
    },
}

impl fmt::Display for ParseBoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongRowCount { actual } => {
                write!(f, "expected {HEIGHT} rows, got {actual}")
            }
            Self::WrongColumnCount { row, actual } => {
                write!(f, "row {row} must contain {WIDTH} cells, got {actual}")
            }
            Self::InvalidCell { row, column, value } => {
                write!(f, "invalid cell {value:?} at row {row}, column {column}")
            }
            Self::FloatingCell { row, column } => {
                write!(f, "floating cell at row {row}, column {column}")
            }
        }
    }
}

impl std::error::Error for ParseBoardError {}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct FieldBits(__m128i);

impl FieldBits {
    #[inline(always)]
    fn zero() -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_setzero_si128()) }
    }

    #[inline(always)]
    fn from_u128(value: u128) -> Self {
        // SAFETY: __m128i is a 128-bit opaque integer vector.
        unsafe { Self(std::mem::transmute::<u128, __m128i>(value)) }
    }

    #[inline(always)]
    fn to_u128(self) -> u128 {
        // SAFETY: __m128i is a 128-bit opaque integer vector.
        unsafe { std::mem::transmute::<__m128i, u128>(self.0) }
    }

    #[inline(always)]
    fn and(self, rhs: Self) -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_and_si128(self.0, rhs.0)) }
    }

    #[inline(always)]
    fn or(self, rhs: Self) -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_or_si128(self.0, rhs.0)) }
    }

    #[inline(always)]
    fn and_not(self, rhs: Self) -> Self {
        // Equivalent to !self & rhs.
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_andnot_si128(self.0, rhs.0)) }
    }

    #[inline(always)]
    fn shift_vertical_up(self) -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_slli_epi16::<1>(self.0)) }
    }

    #[inline(always)]
    fn shift_vertical_down(self) -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_srli_epi16::<1>(self.0)) }
    }

    #[inline(always)]
    fn shift_horizontal_left(self) -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_slli_si128::<2>(self.0)) }
    }

    #[inline(always)]
    fn shift_horizontal_right(self) -> Self {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { Self(_mm_srli_si128::<2>(self.0)) }
    }

    #[inline(always)]
    fn expand_once(self, mask: Self) -> Self {
        self.or(self.shift_vertical_up())
            .or(self.shift_vertical_down())
            .or(self.shift_horizontal_left())
            .or(self.shift_horizontal_right())
            .and(mask)
    }

    #[inline]
    fn expand(self, mask: Self) -> Self {
        let mut seed = self;
        loop {
            let expanded = seed.expand_once(mask);
            if expanded.to_u128() == seed.to_u128() {
                return expanded;
            }
            seed = expanded;
        }
    }

    #[inline(always)]
    fn adjacent(self) -> Self {
        self.shift_vertical_up()
            .or(self.shift_vertical_down())
            .or(self.shift_horizontal_left())
            .or(self.shift_horizontal_right())
    }

    /// Finds every bit belonging to a 4-or-larger orthogonal component.
    #[inline]
    fn vanishing(self) -> Self {
        let up = self.shift_vertical_up().and(self);
        let down = self.shift_vertical_down().and(self);
        let left = self.shift_horizontal_left().and(self);
        let right = self.shift_horizontal_right().and(self);

        let vertical_both = up.and(down);
        let horizontal_both = left.and(right);
        let vertical_either = up.or(down);
        let horizontal_either = left.or(right);

        let degree_three = vertical_both
            .and(horizontal_either)
            .or(horizontal_both.and(vertical_either));
        let degree_two = vertical_both
            .or(horizontal_both)
            .or(vertical_either.and(horizontal_either));

        let connected_degree_two = degree_two
            .shift_vertical_up()
            .and(degree_two)
            .or(degree_two.shift_horizontal_left().and(degree_two));
        let seed = degree_three.or(connected_degree_two);
        if seed.to_u128() == 0 {
            return Self::zero();
        }

        let remaining_degree_two = degree_two
            .shift_vertical_down()
            .and(degree_two)
            .or(degree_two.shift_horizontal_right().and(degree_two));
        seed.or(remaining_degree_two).expand_once(self)
    }
}

#[derive(Clone, Copy)]
pub struct Board {
    planes: [FieldBits; PLANE_COUNT],
}

impl Default for Board {
    fn default() -> Self {
        Self::empty()
    }
}

impl PartialEq for Board {
    fn eq(&self, other: &Self) -> bool {
        self.raw_planes() == other.raw_planes()
    }
}

impl Eq for Board {}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Board").field("rows", &self.rows()).finish()
    }
}

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, row) in self.rows().iter().enumerate() {
            for cell in row {
                write!(f, "{}", cell.as_char())?;
            }
            if index + 1 != HEIGHT {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

impl Board {
    pub fn empty() -> Self {
        Self {
            planes: [FieldBits::zero(); PLANE_COUNT],
        }
    }

    /// Parses 13 top-to-bottom rows separated by `/` or newlines.
    ///
    /// Empty cells are `.`, `E`, or `e`; garbage is `O`; colors are `RGBY`.
    pub fn parse(input: &str) -> Result<Self, ParseBoardError> {
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        let rows = normalized
            .split(['/', '\n'])
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>();
        if rows.len() != HEIGHT {
            return Err(ParseBoardError::WrongRowCount { actual: rows.len() });
        }

        let mut board = Self::empty();
        for (top_row, row) in rows.iter().enumerate() {
            let chars = row.chars().collect::<Vec<_>>();
            if chars.len() != WIDTH {
                return Err(ParseBoardError::WrongColumnCount {
                    row: top_row + 1,
                    actual: chars.len(),
                });
            }
            let y = HEIGHT - 1 - top_row;
            for (x, value) in chars.into_iter().enumerate() {
                let Some(cell) = Cell::from_char(value) else {
                    return Err(ParseBoardError::InvalidCell {
                        row: top_row + 1,
                        column: x + 1,
                        value,
                    });
                };
                board.set(x, y, cell);
            }
        }
        board.validate_gravity()?;
        Ok(board)
    }

    pub fn from_cells(cells: [[Cell; WIDTH]; HEIGHT]) -> Result<Self, ParseBoardError> {
        let mut board = Self::empty();
        for (y, row) in cells.into_iter().enumerate() {
            for (x, cell) in row.into_iter().enumerate() {
                board.set(x, y, cell);
            }
        }
        board.validate_gravity()?;
        Ok(board)
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Cell {
        assert!(x < WIDTH && y < HEIGHT, "cell outside board");
        let bit = cell_bit(x, y);
        let code = self
            .planes
            .iter()
            .enumerate()
            .fold(0u8, |value, (plane, bits)| {
                value | ((((bits.to_u128() & bit) != 0) as u8) << plane)
            });
        match code {
            0b000 => Cell::Empty,
            0b001 => Cell::Garbage,
            0b100 => Cell::Red,
            0b101 => Cell::Green,
            0b110 => Cell::Blue,
            0b111 => Cell::Yellow,
            _ => Cell::Empty,
        }
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        assert!(x < WIDTH && y < HEIGHT, "cell outside board");
        let bit = cell_bit(x, y);
        for plane in 0..PLANE_COUNT {
            let raw = self.planes[plane].to_u128();
            self.planes[plane] = FieldBits::from_u128(if (cell as u8) & (1 << plane) != 0 {
                raw | bit
            } else {
                raw & !bit
            });
        }
    }

    /// Returns top-to-bottom rows, matching [`fmt::Display`] and [`Board::parse`].
    pub fn rows(&self) -> [[Cell; WIDTH]; HEIGHT] {
        let mut result = [[Cell::Empty; WIDTH]; HEIGHT];
        for (top_row, row) in result.iter_mut().enumerate() {
            let y = HEIGHT - 1 - top_row;
            for (x, cell) in row.iter_mut().enumerate() {
                *cell = self.get(x, y);
            }
        }
        result
    }

    pub fn is_empty(&self) -> bool {
        self.occupied_bits().to_u128() == 0
    }

    pub fn validate_gravity(&self) -> Result<(), ParseBoardError> {
        for x in 0..WIDTH {
            let mut found_empty = false;
            for y in 0..HEIGHT {
                if self.get(x, y) == Cell::Empty {
                    found_empty = true;
                } else if found_empty {
                    return Err(ParseBoardError::FloatingCell {
                        row: HEIGHT - y,
                        column: x + 1,
                    });
                }
            }
        }
        Ok(())
    }

    /// Compact column heights derived directly from the six SIMD lanes.
    #[inline]
    pub fn column_heights(&self) -> [u8; WIDTH] {
        let occupied = self.occupied_bits().to_u128();
        std::array::from_fn(|x| {
            let shift = (x + INNER_X_OFFSET) * 16;
            let column = ((occupied >> shift) as u16) & BOARD_COLUMN_MASK;
            if column == 0 { 0 } else { column.ilog2() as u8 }
        })
    }

    /// Resolves one vanish/drop step. `chain_index` is one-based.
    pub fn resolve_step(&mut self, chain_index: u8) -> Option<StepResult> {
        assert!(chain_index >= 1, "chain index is one-based");
        let [p0, p1, p2] = self.planes;
        let visible = FieldBits::from_u128(VISIBLE_MASK);
        let colored = p2.and(visible);
        let not_p0 = p0.and_not(colored);
        let not_p1 = p1.and_not(colored);
        let color_bits = [
            colored.and(not_p0).and(not_p1), // Red
            colored.and(p0).and(not_p1),     // Green
            colored.and(p1).and(not_p0),     // Blue
            colored.and(p0).and(p1),         // Yellow
        ];

        let mut colored_erased = FieldBits::zero();
        let mut color_count = 0u8;
        let mut group_bonus = 0u32;
        let mut waste = 0u32;

        for bits in color_bits {
            let vanishing = bits.vanishing();
            if vanishing.to_u128() == 0 {
                continue;
            }
            color_count += 1;
            colored_erased = colored_erased.or(vanishing);
            for component_size in component_sizes(vanishing, bits) {
                group_bonus += link_bonus(component_size);
                waste += component_size - 4;
            }
        }

        let colored_count = colored_erased.to_u128().count_ones();
        if colored_count == 0 {
            return None;
        }

        let garbage = p0
            .and(p1.and_not(FieldBits::from_u128(BOARD_MASK)))
            .and(p2.and_not(FieldBits::from_u128(BOARD_MASK)));
        let garbage_erased = colored_erased
            .adjacent()
            .and(garbage)
            .and(FieldBits::from_u128(BOARD_MASK));
        let erased = colored_erased.or(garbage_erased);
        let multiplier =
            (chain_bonus(chain_index) + color_bonus(color_count) + group_bonus).clamp(1, 999);
        let score = 10 * colored_count * multiplier;
        let max_drop_distance = self.remove_and_compact(erased);

        Some(StepResult {
            chain_index,
            score,
            colored_erased: colored_count,
            garbage_erased: garbage_erased.to_u128().count_ones(),
            colors_erased: color_count,
            group_bonus,
            waste,
            max_drop_distance,
            erased_mask: erased.to_u128(),
        })
    }

    /// Resolves all chains until stable.
    pub fn simulate(&mut self) -> SimulationResult {
        let mut result = SimulationResult::default();
        for chain_index in 1..=u8::MAX {
            let Some(step) = self.resolve_step(chain_index) else {
                break;
            };
            result.score += u64::from(step.score);
            result.steps.push(step);
        }
        result.all_clear = self.is_empty();
        result
    }

    /// Allocation-free variant intended for tree search and neural-network data generation.
    pub fn simulate_summary(&mut self) -> SimulationSummary {
        let mut result = SimulationSummary::default();
        for chain_index in 1..=u8::MAX {
            let Some(step) = self.resolve_step(chain_index) else {
                break;
            };
            result.score += u64::from(step.score);
            result.chains += 1;
            result.colored_erased += step.colored_erased;
            result.garbage_erased += step.garbage_erased;
            result.max_drop_distance = result.max_drop_distance.max(step.max_drop_distance);
        }
        result.all_clear = self.is_empty();
        result
    }

    /// Minimal chain resolver for MCTS branches. It deliberately omits score,
    /// group/waste bonuses, erased counts, and drop-distance bookkeeping.
    #[inline]
    pub fn simulate_chain_count(&mut self) -> u8 {
        let mut chains = 0u8;
        while self.resolve_chain_step() {
            chains = chains.saturating_add(1);
        }
        chains
    }

    /// Produces dense, simulator-authored supervision for the neural network.
    ///
    /// Indices use `y * WIDTH + x`, with `y = 0` at the floor. A vanish step of
    /// zero means that the original cell survives (or that the cell was empty).
    /// Fall distance follows each original cell through every compaction.
    pub fn chain_labels(&self) -> ChainLabels {
        #[derive(Clone, Copy, Default)]
        struct TrackedCell {
            origin: usize,
        }

        let mut board = self.clone();
        let mut columns = [[None::<TrackedCell>; HEIGHT]; WIDTH];
        let mut lengths = [0usize; WIDTH];
        for x in 0..WIDTH {
            for y in 0..HEIGHT {
                if self.get(x, y) != Cell::Empty {
                    columns[x][lengths[x]] = Some(TrackedCell {
                        origin: y * WIDTH + x,
                    });
                    lengths[x] += 1;
                }
            }
        }

        let mut result = ChainLabels {
            vanish_step: [0; CELL_COUNT],
            fall_distance: [0; CELL_COUNT],
            component_size: self.connected_component_sizes(),
            chains: 0,
        };

        for chain in 1..=u8::MAX {
            let Some(step) = board.resolve_step(chain) else {
                break;
            };
            result.chains = chain;
            for x in 0..WIDTH {
                let old_length = lengths[x];
                let mut destination = 0usize;
                for source in 0..old_length {
                    let tracked = columns[x][source].expect("tracked column is compact");
                    if step.erased_mask & cell_bit(x, source) != 0 {
                        result.vanish_step[tracked.origin] = chain;
                    } else {
                        result.fall_distance[tracked.origin] += (source - destination) as u8;
                        columns[x][destination] = Some(tracked);
                        destination += 1;
                    }
                }
                for slot in columns[x].iter_mut().take(old_length).skip(destination) {
                    *slot = None;
                }
                lengths[x] = destination;
            }
        }
        result
    }

    /// Size of the same-colour orthogonal component containing each cell.
    /// Empty and garbage cells are labelled zero.
    pub fn connected_component_sizes(&self) -> [u8; CELL_COUNT] {
        let mut result = [0u8; CELL_COUNT];
        for color in Cell::COLORS {
            let color_bits = self.bits_for(color).and(FieldBits::from_u128(BOARD_MASK));
            let mut remaining = color_bits.to_u128();
            while remaining != 0 {
                let seed = FieldBits::from_u128(1u128 << remaining.trailing_zeros());
                let component = seed.expand(color_bits);
                let raw = component.to_u128();
                let size = raw.count_ones().min(u32::from(u8::MAX)) as u8;
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        if raw & cell_bit(x, y) != 0 {
                            result[y * WIDTH + x] = size;
                        }
                    }
                }
                remaining &= !raw;
            }
        }
        result
    }

    fn raw_planes(&self) -> [u128; PLANE_COUNT] {
        self.planes.map(FieldBits::to_u128)
    }

    #[inline(always)]
    fn occupied_bits(&self) -> FieldBits {
        self.planes[0].or(self.planes[1]).or(self.planes[2])
    }

    #[inline(always)]
    fn bits_for(&self, cell: Cell) -> FieldBits {
        let mut result = FieldBits::from_u128(BOARD_MASK);
        for plane in 0..PLANE_COUNT {
            result = if (cell as u8) & (1 << plane) != 0 {
                result.and(self.planes[plane])
            } else {
                self.planes[plane].and_not(result)
            };
        }
        result
    }

    fn remove_and_compact(&mut self, erased: FieldBits) -> u8 {
        self.remove_and_compact_inner::<true>(erased)
    }

    #[inline]
    fn resolve_chain_step(&mut self) -> bool {
        let [p0, p1, p2] = self.planes;
        let visible = FieldBits::from_u128(VISIBLE_MASK);
        // Colour cells all have plane 2 set; Red/Green/Blue/Yellow differ only
        // in planes 0 and 1. Computing the four colour masks directly avoids
        // the generic `bits_for` dispatch in the hottest loop.
        let colored = p2.and(visible);
        let not_p0 = p0.and_not(colored);
        let not_p1 = p1.and_not(colored);
        let red = colored.and(not_p0).and(not_p1);
        let green = colored.and(p0).and(not_p1);
        let blue = colored.and(p1).and(not_p0);
        let yellow = colored.and(p0).and(p1);
        let colored_erased = red
            .vanishing()
            .or(green.vanishing())
            .or(blue.vanishing())
            .or(yellow.vanishing());
        if colored_erased.to_u128() == 0 {
            return false;
        }
        let garbage = p0
            .and(p1.and_not(FieldBits::from_u128(BOARD_MASK)))
            .and(p2.and_not(FieldBits::from_u128(BOARD_MASK)));
        let garbage_erased = colored_erased.adjacent().and(garbage);
        self.remove_and_compact_inner::<false>(colored_erased.or(garbage_erased));
        true
    }

    #[inline]
    fn remove_and_compact_inner<const TRACK_DROP: bool>(&mut self, erased: FieldBits) -> u8 {
        let erased_raw = erased.to_u128();
        let occupied_raw = self.occupied_bits().to_u128();
        let mut max_drop = 0u8;

        if TRACK_DROP {
            for x in 0..WIDTH {
                let shift = (x + INNER_X_OFFSET) * 16;
                let erased_column = ((erased_raw >> shift) as u16) & BOARD_COLUMN_MASK;
                let occupied_column = ((occupied_raw >> shift) as u16) & !erased_column;
                if occupied_column != 0 {
                    // The highest survivor has every erased cell that can
                    // affect any survivor below it, so one popcount is enough.
                    let top = occupied_column.ilog2();
                    let below = erased_column & ((1u16 << top) - 1);
                    max_drop = max_drop.max(below.count_ones() as u8);
                }
            }
        }
        compact_planes(&mut self.planes, erased_raw);
        max_drop
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    pub chain_index: u8,
    pub score: u32,
    pub colored_erased: u32,
    pub garbage_erased: u32,
    pub colors_erased: u8,
    pub group_bonus: u32,
    pub waste: u32,
    pub max_drop_distance: u8,
    /// Internal PDF-compatible bit layout; useful for differential verification.
    pub erased_mask: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimulationResult {
    pub score: u64,
    pub all_clear: bool,
    pub steps: Vec<StepResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimulationSummary {
    pub score: u64,
    pub chains: u8,
    pub colored_erased: u32,
    pub garbage_erased: u32,
    pub max_drop_distance: u8,
    pub all_clear: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainLabels {
    pub vanish_step: [u8; CELL_COUNT],
    pub fall_distance: [u8; CELL_COUNT],
    pub component_size: [u8; CELL_COUNT],
    pub chains: u8,
}

impl SimulationResult {
    pub fn chains(&self) -> usize {
        self.steps.len()
    }
}

#[inline(always)]
const fn cell_bit(x: usize, y: usize) -> u128 {
    1u128 << ((x + INNER_X_OFFSET) * 16 + y + INNER_Y_OFFSET)
}

fn component_sizes(bits: FieldBits, connectivity_mask: FieldBits) -> ComponentSizes {
    ComponentSizes {
        remaining: bits.to_u128(),
        connectivity_mask,
    }
}

struct ComponentSizes {
    remaining: u128,
    connectivity_mask: FieldBits,
}

impl Iterator for ComponentSizes {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let seed = FieldBits::from_u128(1u128 << self.remaining.trailing_zeros());
        let component = seed.expand(self.connectivity_mask);
        let raw = component.to_u128();
        self.remaining &= !raw;
        Some(raw.count_ones())
    }
}

/// Dispatches to the BMI2 `PEXT` compaction once, then caches the choice in a
/// function pointer. The first call pays the feature-detection cost; every
/// later call is a single indirect jump.
fn compact_planes(planes: &mut [FieldBits; PLANE_COUNT], erased: u128) {
    type Impl = unsafe fn(&mut [FieldBits; PLANE_COUNT], u128);
    static IMPL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    unsafe fn detect(planes: &mut [FieldBits; PLANE_COUNT], erased: u128) {
        let implementation: Impl = if std::arch::is_x86_feature_detected!("bmi2") {
            compact_planes_bmi2
        } else {
            compact_planes_scalar
        };
        IMPL.store(implementation as usize, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: `implementation` is a valid function pointer for this signature.
        unsafe { implementation(planes, erased) };
    }

    let raw = IMPL.load(std::sync::atomic::Ordering::Relaxed);
    // SAFETY: `raw` is either 0 (uninitialised) or a function pointer stored by
    // `detect`. When 0 we use `detect`, which performs the one-time dispatch.
    let implementation: Impl = if raw == 0 {
        detect
    } else {
        unsafe { std::mem::transmute::<usize, Impl>(raw) }
    };
    // SAFETY: `implementation` always points to a valid compaction routine.
    unsafe { implementation(planes, erased) };
}

#[cfg(test)]
#[target_feature(enable = "bmi2")]
unsafe fn compact_column_bmi2(column: u16, keep: u16) -> u16 {
    let packed = _pext_u32(u32::from(column), u32::from(keep));
    (packed << INNER_Y_OFFSET) as u16
}

#[target_feature(enable = "bmi2")]
unsafe fn compact_planes_bmi2(planes: &mut [FieldBits; PLANE_COUNT], erased: u128) {
    for x in 0..WIDTH {
        let shift = (x + INNER_X_OFFSET) * 16;
        let erased_column = ((erased >> shift) as u16) & BOARD_COLUMN_MASK;
        let keep = BOARD_COLUMN_MASK & !erased_column;
        let lane_mask = (u16::MAX as u128) << shift;
        for plane in &mut *planes {
            let raw = plane.to_u128();
            let column = (raw >> shift) as u16;
            let packed = (_pext_u32(u32::from(column), u32::from(keep)) << INNER_Y_OFFSET) as u16;
            *plane = FieldBits::from_u128((raw & !lane_mask) | ((packed as u128) << shift));
        }
    }
}

unsafe fn compact_planes_scalar(planes: &mut [FieldBits; PLANE_COUNT], erased: u128) {
    for x in 0..WIDTH {
        let shift = (x + INNER_X_OFFSET) * 16;
        let erased_column = ((erased >> shift) as u16) & BOARD_COLUMN_MASK;
        let keep = BOARD_COLUMN_MASK & !erased_column;
        let lane_mask = (u16::MAX as u128) << shift;
        for plane in &mut *planes {
            let raw = plane.to_u128();
            let column = (raw >> shift) as u16;
            let packed = compact_column_scalar(column, keep);
            *plane = FieldBits::from_u128((raw & !lane_mask) | ((packed as u128) << shift));
        }
    }
}

fn compact_column_scalar(column: u16, keep: u16) -> u16 {
    let mut source = column & keep;
    let mut mask = keep;
    let mut packed = 0u16;
    let mut destination = INNER_Y_OFFSET;
    while mask != 0 {
        let bit = mask.trailing_zeros();
        if source & (1 << bit) != 0 {
            packed |= 1 << destination;
        }
        mask &= mask - 1;
        source &= !(1 << bit);
        destination += 1;
    }
    packed
}

const fn color_bonus(colors: u8) -> u32 {
    match colors {
        1 => 0,
        2 => 3,
        3 => 6,
        4 => 12,
        _ => unreachable!(),
    }
}

const fn link_bonus(size: u32) -> u32 {
    match size {
        0..=4 => 0,
        5 => 2,
        6 => 3,
        7 => 4,
        8 => 5,
        9 => 6,
        10 => 7,
        _ => 10,
    }
}

const fn chain_bonus(chain: u8) -> u32 {
    const BONUS: [u32; 19] = [
        0, 8, 16, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512,
    ];
    if chain as usize <= BONUS.len() {
        BONUS[chain as usize - 1]
    } else {
        512
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ScalarBoard([[Cell; WIDTH]; HEIGHT]);

    #[derive(Debug, PartialEq, Eq)]
    struct ScalarStep {
        score: u32,
        colored_erased: u32,
        garbage_erased: u32,
        colors_erased: u8,
        group_bonus: u32,
        waste: u32,
        max_drop_distance: u8,
        erased_mask: u128,
    }

    impl ScalarBoard {
        fn from_fast(board: &Board) -> Self {
            let mut cells = [[Cell::Empty; WIDTH]; HEIGHT];
            for (y, row) in cells.iter_mut().enumerate() {
                for (x, cell) in row.iter_mut().enumerate() {
                    *cell = board.get(x, y);
                }
            }
            Self(cells)
        }

        #[allow(clippy::needless_range_loop)]
        fn resolve_step(&mut self, chain: u8) -> Option<ScalarStep> {
            let mut visited = [[false; WIDTH]; VISIBLE_HEIGHT];
            let mut erased = [[false; WIDTH]; HEIGHT];
            let mut colors_erased = 0u8;
            let mut colored_erased = 0u32;
            let mut group_bonus = 0u32;
            let mut waste = 0u32;

            for color in Cell::COLORS {
                let mut color_vanished = false;
                for y in 0..VISIBLE_HEIGHT {
                    for x in 0..WIDTH {
                        if visited[y][x] || self.0[y][x] != color {
                            continue;
                        }
                        let mut queue = VecDeque::from([(x, y)]);
                        let mut component = Vec::new();
                        visited[y][x] = true;
                        while let Some((cx, cy)) = queue.pop_front() {
                            component.push((cx, cy));
                            for (nx, ny) in neighbors(cx, cy) {
                                if ny < VISIBLE_HEIGHT
                                    && !visited[ny][nx]
                                    && self.0[ny][nx] == color
                                {
                                    visited[ny][nx] = true;
                                    queue.push_back((nx, ny));
                                }
                            }
                        }
                        if component.len() >= 4 {
                            color_vanished = true;
                            colored_erased += component.len() as u32;
                            group_bonus += link_bonus(component.len() as u32);
                            waste += component.len() as u32 - 4;
                            for (cx, cy) in component {
                                erased[cy][cx] = true;
                            }
                        }
                    }
                }
                colors_erased += u8::from(color_vanished);
            }
            if colored_erased == 0 {
                return None;
            }

            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    if !erased[y][x] || self.0[y][x] == Cell::Garbage {
                        continue;
                    }
                    for (nx, ny) in neighbors(x, y) {
                        if self.0[ny][nx] == Cell::Garbage {
                            erased[ny][nx] = true;
                        }
                    }
                }
            }

            let mut garbage_erased = 0u32;
            let mut erased_mask = 0u128;
            let mut max_drop_distance = 0u8;
            for x in 0..WIDTH {
                for y in 0..HEIGHT {
                    if erased[y][x] {
                        erased_mask |= cell_bit(x, y);
                        garbage_erased += u32::from(self.0[y][x] == Cell::Garbage);
                    } else if self.0[y][x] != Cell::Empty {
                        let below = (0..y).filter(|&below_y| erased[below_y][x]).count() as u8;
                        max_drop_distance = max_drop_distance.max(below);
                    }
                }

                let mut destination = 0;
                for source in 0..HEIGHT {
                    if !erased[source][x] {
                        self.0[destination][x] = self.0[source][x];
                        destination += 1;
                    }
                }
                while destination < HEIGHT {
                    self.0[destination][x] = Cell::Empty;
                    destination += 1;
                }
            }
            let multiplier =
                (chain_bonus(chain) + color_bonus(colors_erased) + group_bonus).clamp(1, 999);
            Some(ScalarStep {
                score: 10 * colored_erased * multiplier,
                colored_erased,
                garbage_erased,
                colors_erased,
                group_bonus,
                waste,
                max_drop_distance,
                erased_mask,
            })
        }
    }

    fn neighbors(x: usize, y: usize) -> impl Iterator<Item = (usize, usize)> {
        [
            x.checked_sub(1).map(|nx| (nx, y)),
            (x + 1 < WIDTH).then_some((x + 1, y)),
            y.checked_sub(1).map(|ny| (x, ny)),
            (y + 1 < HEIGHT).then_some((x, y + 1)),
        ]
        .into_iter()
        .flatten()
    }

    fn board_from_columns(columns: &[&str; WIDTH]) -> Board {
        let mut board = Board::empty();
        for (x, column) in columns.iter().enumerate() {
            for (y, value) in column.chars().enumerate() {
                board.set(x, y, Cell::from_char(value).unwrap());
            }
        }
        board.validate_gravity().unwrap();
        board
    }

    fn assert_step_matches_scalar(mut board: Board, chain: u8) {
        let mut scalar = ScalarBoard::from_fast(&board);
        let fast = board.resolve_step(chain);
        let slow = scalar.resolve_step(chain);
        match (fast, slow) {
            (None, None) => {}
            (Some(fast), Some(slow)) => {
                assert_eq!(fast.score, slow.score);
                assert_eq!(fast.colored_erased, slow.colored_erased);
                assert_eq!(fast.garbage_erased, slow.garbage_erased);
                assert_eq!(fast.colors_erased, slow.colors_erased);
                assert_eq!(fast.group_bonus, slow.group_bonus);
                assert_eq!(fast.waste, slow.waste);
                assert_eq!(fast.max_drop_distance, slow.max_drop_distance);
                assert_eq!(fast.erased_mask, slow.erased_mask);
                assert_eq!(ScalarBoard::from_fast(&board), scalar);
            }
            mismatch => panic!("fast/scalar mismatch: {mismatch:?}"),
        }
    }

    #[test]
    fn parser_round_trip_and_gravity_validation() {
        let text = "....../....../....../....../....../....../....../....../....../....../....../R...../RG....";
        let board = Board::parse(text).unwrap();
        assert_eq!(board.to_string().replace('\n', "/"), text);
        let floating = "....../....../....../....../....../....../....../....../....../....../R...../....../......";
        assert!(matches!(
            Board::parse(floating),
            Err(ParseBoardError::FloatingCell { .. })
        ));
    }

    #[test]
    fn four_vertical_puyos_score_40_and_all_clear() {
        let mut board = board_from_columns(&["RRRR", "", "", "", "", ""]);
        let result = board.simulate();
        assert_eq!(result.chains(), 1);
        assert_eq!(result.score, 40);
        assert!(result.all_clear);
    }

    #[test]
    fn hidden_thirteenth_row_never_participates_in_vanishing() {
        let mut board = board_from_columns(&["BGBGBGBGBRRRR", "", "", "", "", ""]);
        assert!(board.resolve_step(1).is_none());
    }

    #[test]
    fn adjacent_garbage_is_erased_but_diagonal_garbage_survives() {
        let mut board = board_from_columns(&["RRRR", "O", "", "", "", ""]);
        board.set(1, 4, Cell::Garbage);
        // Restore the gravity invariant by filling below the diagonal garbage.
        for y in 1..4 {
            board.set(1, y, Cell::Blue);
        }
        let step = board.resolve_step(1).unwrap();
        assert_eq!(step.garbage_erased, 1);
        assert_eq!(board.get(1, 3), Cell::Garbage);
    }

    #[test]
    fn disconnected_groups_do_not_inflate_waste_or_link_bonus() {
        let mut board = board_from_columns(&["RRRR", "", "", "", "", "RRRR"]);
        let step = board.resolve_step(1).unwrap();
        assert_eq!(step.colored_erased, 8);
        assert_eq!(step.group_bonus, 0);
        assert_eq!(step.waste, 0);
        assert_eq!(step.score, 80);
    }

    #[test]
    fn max_drop_distance_ignores_erased_cells_with_nothing_above() {
        let mut top_only = board_from_columns(&["RRRR", "", "", "", "", ""]);
        assert_eq!(top_only.resolve_step(1).unwrap().max_drop_distance, 0);

        let mut falling = board_from_columns(&["RRRRB", "", "", "", "", ""]);
        assert_eq!(falling.resolve_step(1).unwrap().max_drop_distance, 4);
        assert_eq!(falling.get(0, 0), Cell::Blue);
    }

    #[test]
    fn two_chain_fixture_resolves_in_order() {
        let mut board = board_from_columns(&["RRRBG", "RBBBG", "GGG", "", "", ""]);
        let mut summary_board = board.clone();
        let result = board.simulate();
        let summary = summary_board.simulate_summary();
        assert_eq!(result.chains(), 2);
        assert!(result.score > result.steps[0].score as u64);
        assert_eq!(summary.chains, result.chains() as u8);
        assert_eq!(summary.score, result.score);
        assert_eq!(summary.all_clear, result.all_clear);
        assert_eq!(summary_board, board);
    }

    #[test]
    fn chain_labels_follow_original_cells_across_compaction() {
        let board = board_from_columns(&["RRRBG", "RBBBG", "GGG", "", "", ""]);
        let labels = board.chain_labels();
        let mut simulated = board.clone();
        let result = simulated.simulate();

        assert_eq!(labels.chains as usize, result.chains());
        assert_eq!(labels.vanish_step.iter().copied().max(), Some(2));
        assert!(labels.fall_distance.iter().any(|&distance| distance > 0));
        for step in &result.steps {
            let labelled = labels
                .vanish_step
                .iter()
                .filter(|&&chain| chain == step.chain_index)
                .count() as u32;
            assert_eq!(labelled, step.colored_erased + step.garbage_erased);
        }
    }

    #[test]
    fn component_labels_are_spatial_and_ignore_garbage() {
        let board = board_from_columns(&["RRRR", "R", "GG", "G", "O", "B"]);
        let labels = board.connected_component_sizes();
        assert_eq!(labels[0], 5);
        assert_eq!(labels[3 * WIDTH], 5);
        assert_eq!(labels[1], 5);
        assert_eq!(labels[2], 3);
        assert_eq!(labels[WIDTH + 2], 3);
        assert_eq!(labels[3], 3);
        assert_eq!(labels[4], 0);
        assert_eq!(labels[5], 1);
    }

    #[test]
    fn every_four_by_four_color_shape_matches_scalar_reference() {
        for pattern in 0u32..(1 << 16) {
            let mut board = Board::empty();
            for index in 0..16 {
                if pattern & (1 << index) != 0 {
                    board.set(index % 4, index / 4, Cell::Red);
                }
            }
            // The erasure detector itself supports arbitrary shapes, even though
            // public simulation requires gravity-settled input.
            let fast = board
                .bits_for(Cell::Red)
                .and(FieldBits::from_u128(VISIBLE_MASK))
                .vanishing()
                .to_u128();
            let scalar = scalar_vanishing_mask(&board, Cell::Red);
            assert_eq!(fast, scalar, "pattern={pattern:#018b}");
        }
    }

    fn scalar_vanishing_mask(board: &Board, color: Cell) -> u128 {
        let mut visited = [[false; WIDTH]; VISIBLE_HEIGHT];
        let mut result = 0u128;
        for y in 0..VISIBLE_HEIGHT {
            for x in 0..WIDTH {
                if visited[y][x] || board.get(x, y) != color {
                    continue;
                }
                let mut queue = VecDeque::from([(x, y)]);
                let mut component = Vec::new();
                visited[y][x] = true;
                while let Some((cx, cy)) = queue.pop_front() {
                    component.push((cx, cy));
                    for (nx, ny) in neighbors(cx, cy) {
                        if ny < VISIBLE_HEIGHT && !visited[ny][nx] && board.get(nx, ny) == color {
                            visited[ny][nx] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }
                if component.len() >= 4 {
                    for (cx, cy) in component {
                        result |= cell_bit(cx, cy);
                    }
                }
            }
        }
        result
    }

    fn next_random(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn one_hundred_thousand_adversarial_boards_match_scalar_reference() {
        let mut random = 0xd1b5_4a32_d192_ed03u64;
        for case in 0..100_000 {
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
            assert_step_matches_scalar(board, (case % 24 + 1) as u8);
        }
    }

    #[test]
    fn one_hundred_thousand_chain_only_resolutions_match_full_summary() {
        let mut random = 0x243f_6a88_85a3_08d3u64;
        for case in 0..100_000 {
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
            let mut full = board.clone();
            let mut minimal = board;
            let expected = full.simulate_summary().chains;
            let actual = minimal.simulate_chain_count();
            assert_eq!(actual, expected, "case={case}");
            assert_eq!(minimal, full, "case={case}");
        }
    }

    #[test]
    fn ten_thousand_adversarial_label_traces_preserve_cell_identity() {
        let mut random = 0x6a09_e667_f3bc_c909u64;
        for case in 0..10_000 {
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

            let labels = board.chain_labels();
            let mut final_board = board.clone();
            let simulation = final_board.simulate();
            assert_eq!(labels.chains as usize, simulation.chains(), "case={case}");
            for step in &simulation.steps {
                let count = labels
                    .vanish_step
                    .iter()
                    .filter(|&&value| value == step.chain_index)
                    .count() as u32;
                assert_eq!(
                    count,
                    step.colored_erased + step.garbage_erased,
                    "case={case}"
                );
            }
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let index = y * WIDTH + x;
                    let original = board.get(x, y);
                    if original == Cell::Empty {
                        assert_eq!(labels.vanish_step[index], 0, "case={case}");
                        assert_eq!(labels.fall_distance[index], 0, "case={case}");
                    } else if labels.vanish_step[index] == 0 {
                        let final_y = y - usize::from(labels.fall_distance[index]);
                        assert_eq!(final_board.get(x, final_y), original, "case={case}");
                    }
                }
            }
        }
    }

    #[test]
    fn bmi2_and_scalar_compaction_match_on_one_million_inputs() {
        if !std::arch::is_x86_feature_detected!("bmi2") {
            return;
        }
        let mut random = 0xa076_1d64_78bd_642fu64;
        for _ in 0..1_000_000 {
            let column = next_random(&mut random) as u16;
            let erased = next_random(&mut random) as u16 & BOARD_COLUMN_MASK;
            let keep = BOARD_COLUMN_MASK & !erased;
            let scalar = compact_column_scalar(column, keep);
            // SAFETY: guarded by runtime detection above.
            let bmi2 = unsafe { compact_column_bmi2(column, keep) };
            assert_eq!(bmi2, scalar);
        }
    }
}
