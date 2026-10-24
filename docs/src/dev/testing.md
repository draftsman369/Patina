# Testing

```sh
cargo test --workspace
```

## Unit tests

Each crate tests its own logic: image and texture set handling, mesh import
from in-memory OBJ/glTF fixtures, primitive validity (including triangle
winding), normal/tangent generation, camera framing, mip generation, UV view
transforms and CLI parsing.

## GPU snapshot tests

`crates/patina_render/tests/snapshot.rs` renders fixed scenes at 256×256 and
compares them with `tests/snapshots/*.png`.

The comparison matches each pixel against its best neighbor in a 3×3 window
of the reference, so 1-pixel edge shifts between rasterizers don't fail the
test. A test fails when the mean difference exceeds 2/255 or too many pixels
differ by more than 24/255 (0.5% for shaded images, 3% for line-heavy ones).

| Variable | Effect |
|---|---|
| `PATINA_UPDATE_SNAPSHOTS=1` | Rewrite references |
| `PATINA_SOFTWARE_GPU=1` | Software adapter (lavapipe on Linux, WARP on Windows) |
| `PATINA_REQUIRE_GPU=1` | Fail instead of skipping without a GPU |

On failure the actual image and a red diff mask are written to
`target/snapshot-failures/`; CI uploads that folder as an artifact.
