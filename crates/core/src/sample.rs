//! A small single-storey house built from commands: the demo (`bimcraft-cli run --sample`) and
//! a test of the whole command set.

pub const SAMPLE: &str = r#"# Sample house: 10 x 7 m, four rooms under a hipped roof, dimensioned and tagged, with elevations, a section, and three sheets.
new {"name": "Sample House"}
grid {"start": [0, -1500], "end": [0, 8500]}
grid {"start": [6000, -1500], "end": [6000, 8500]}
grid {"start": [10000, -1500], "end": [10000, 8500]}
grid {"start": [-1500, 0], "end": [11500, 0], "name": "A"}
grid {"start": [-1500, 7000], "end": [11500, 7000]}
wall {"points": [[0, 0], [10000, 0], [10000, 7000], [0, 7000]], "closed": true, "type": "Exterior - 300mm"}
wall {"points": [[6000, 0], [6000, 7000]], "type": "Interior - 100mm"}
wall {"points": [[0, 4000], [6000, 4000]], "type": "Interior - 100mm"}
wall {"points": [[6000, 3500], [10000, 3500]], "type": "Interior - 100mm"}
door {"at": [2000, 0], "flip_facing": true}
door {"at": [4500, 4000], "type": "Single 800 x 2100"}
door {"at": [6000, 1750], "type": "Single 800 x 2100", "flip_hand": true}
door {"at": [6000, 5250], "type": "Single 800 x 2100", "flip_facing": true}
window {"at": [4300, 0]}
window {"at": [3000, 7000]}
window {"at": [8000, 7000]}
window {"at": [10000, 5250], "type": "Fixed 600 x 900"}
window {"at": [8000, 0]}
floor {}
roof {}
room {"point": [3000, 2000], "name": "Living"}
room {"point": [3000, 5500], "name": "Kitchen"}
room {"point": [8000, 5250], "name": "Bedroom"}
room {"point": [8000, 1750], "name": "Study"}
# Overall sizes outside the walls, the grids' spacing, and every door and window tagged.
dimension {"refs": [[-150, 3500], [10150, 3500]], "at": [5000, -1300]}
dimension {"refs": [[5000, -150], [5000, 7150]], "at": [10900, 3500]}
dimension {"refs": [[0, 8000], [6000, 8000], [10000, 8000]], "at": [5000, 8000]}
tag {}
# The four building elevations, and a section across the living room and the study, looking north.
elevation {}
section {"start": [-1500, 3000], "end": [11500, 3000]}
# Two sheets: the ground floor plan, and the section with two of the elevations.
sheet {"name": "Ground Floor"}
place {"sheet": "A101", "view": "Level 1"}
sheet {"name": "Section and Elevations"}
place {"sheet": "A102", "view": "Section 1", "at": [120, 215]}
place {"sheet": "A102", "view": "South", "at": [120, 110]}
place {"sheet": "A102", "view": "East", "at": [300, 160]}
# A note and a spot elevation on the ground floor, the room schedule beside its plan, and the 3D view on a sheet of its own.
text {"text": "Oak floor", "at": [6300, 900]}
spot {"at": [1000, 1000]}
place {"sheet": "A101", "view": "Room Schedule"}
sheet {"name": "3D View"}
place {"sheet": "A103", "view": "{3D}", "scale": 75}
"#;
