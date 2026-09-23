# meili_puyo

Puyo Puyo AI project. Rust workspace with a fast SIMD chain simulator,
chance-PUCT search, an interactive CUI, and a Web GUI.

## Layout

```
crates/puyo-core   Board (3x128-bit planes, SSE2/BMI2), chain resolution,
                   legal placements, Modern128 queue, GameState
crates/puyo-ai     Chance-PUCT MCTS over public state, pluggable Evaluator
apps/puyo-cli      Command line front-end (play / simulate / bench / selfplay)
apps/puyo-web      Web GUI + JSON API (axum, native SIMD backend)
```

## Quick start

```bash
cargo build --release

# Interactive CUI (SIMULATIONS>0 shows AI hint)
./target/release/puyo-cli play [SEED] [SIMULATIONS]

# Web GUI (listens on 0.0.0.0 so Windows can reach it via localhost)
./target/release/puyo-web          # http://localhost:8791
PUYO_WEB_ADDR=127.0.0.1:9000 ./target/release/puyo-web
```

## CLI commands

```bash
puyo-cli play [SEED] [SIMULATIONS]     Interactive CUI play
puyo-cli simulate [--board ROWS|--stdin]  Resolve chains on a board
puyo-cli suggest [SEED] [SIMULATIONS]  Print MCTS suggestion
puyo-cli bench [ITERATIONS]            Chain simulation benchmark
puyo-cli mcts-bench [SEARCHES] [SIMS]  MCTS benchmark
puyo-cli selfplay OUT.jsonl [GAMES] [SIMS] [MAX_MOVES]
```

### play keys

| key | action |
|---|---|
| ←/→ or h/l | move |
| ↓ or j | soft drop |
| z / x | rotate CCW / CW |
| c | rotate 180 |
| space | hard drop |
| q | quit |

### simulate

Board is 13 top-to-bottom rows of 6 cells: `.` empty, `O` garbage,
`R/G/B/Y` colors, rows separated by `/` or newlines.

## Web API

| endpoint | method | description |
|---|---|---|
| `/api/health` | GET | engine status (SSE2/BMI2) |
| `/api/simulate` | POST | resolve chains, returns per-step frames |
| `/api/game` | POST | game state, legal actions with routes, apply a placement |
| `/api/search` | POST | chance-PUCT root visit distribution |

## Performance (Ryzen, release build)

| operation | time |
|---|---|
| full chain simulation | ~51 ns |
| chain count only | ~28 ns |
| legal placements (compact) | ~32 ns |
| MCTS 800 simulations | ~0.38 ms |

## Notes

- x86_64 only (SSE2 baseline, BMI2 `PEXT` for compaction when available).
- Row 13 (top row) is hidden: it can hold/fall puyos but never vanishes.
- Death is checked at column 2, row 11 (0-indexed `DEATH_X=2, DEATH_Y=11`).
- The MCTS tree owns only public information; the hidden queue seed never
  enters the search.
