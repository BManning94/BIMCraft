//! IFC4 export (ISO 16739 as an ISO 10303-21 STEP file), the open BIM exchange format every BIM
//! tool reads. Levels become building storeys; walls (their plan outline, mitred where they join,
//! curved or straight), slabs, spaces, columns and stairs get swept-solid bodies, roofs and walls
//! attached to them closed shells of flat faces; each door and window fills an opening element that
//! voids its wall, so readers cut the hole; compound walls carry their layers as a material layer set.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::geom::{self, P2};
use crate::model::{DEFAULT_HEIGHT, Element, Function, Id, Model, heading, mix};
use crate::walls::{self, Axis};

// What a GlobalId is for, so each one stays the same from export to export.
const PROJECT: u64 = 1;
const SITE: u64 = 2;
const BUILDING: u64 = 3;
const STOREY: u64 = 4;
const ELEMENT: u64 = 5;
const OPENING: u64 = 6;
const VOIDS: u64 = 7;
const FILLS: u64 = 8;
const AGGREGATES: u64 = 9;
const CONTAINS: u64 = 10;
const SPACES: u64 = 11;
const MATERIALS: u64 = 12;

/// A STEP string literal: `'` doubled, `\` doubled, anything outside printable ASCII as `\X2\…\X0\`.
fn s(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str(r"\\"),
            ' '..='~' => out.push(c),
            _ => {
                out.push_str(r"\X2\");
                for unit in c.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "{unit:04X}");
                }
                out.push_str(r"\X0\");
            }
        }
    }
    out.push('\'');
    out
}

/// A STEP real: always with a decimal point.
fn real(x: f64) -> String {
    let x = if x.is_finite() && x != 0.0 { x } else { 0.0 };
    format!("{x:.6}").trim_end_matches('0').to_string()
}

/// A 22-character IFC GlobalId, the same for the same model, role and element on every export.
fn guid(uid: u64, role: u64, id: u64) -> String {
    const CHARS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    let hi = mix(uid ^ mix(role.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ id));
    let lo = mix(hi ^ id.rotate_left(17) ^ role);
    let v = (u128::from(hi) << 64) | u128::from(lo);
    // 128 bits as 22 base-64 digits: the first carries the top 2 bits.
    let digit = |shift: u32| char::from(CHARS[((v >> shift) & 63) as usize]);
    std::iter::once(digit(126)).chain((0..21).rev().map(|i| digit(6 * i))).collect()
}

/// UTC date and time of a Unix time, ISO 8601.
fn utc(secs: u64) -> String {
    // Days to civil date: Howard Hinnant's algorithm.
    let z = (secs / 86_400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", t / 3600, t % 3600 / 60, t % 60)
}

fn refs(ids: &[u64]) -> String {
    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",")
}

struct Step {
    out: String,
    next: u64,
    uid: u64,
    /// Shared: the origin placement, +Z, and the body context.
    world: u64,
    z: u64,
    body: u64,
}

impl Step {
    /// Writes `#n=ENTITY;` and returns n.
    fn add(&mut self, entity: impl AsRef<str>) -> u64 {
        self.next += 1;
        let _ = writeln!(self.out, "#{}={};", self.next, entity.as_ref());
        self.next
    }

    fn guid(&self, role: u64, id: u64) -> String {
        s(&guid(self.uid, role, id))
    }

    fn point(&mut self, p: P2) -> u64 {
        self.add(format!("IFCCARTESIANPOINT(({},{}))", real(p.0), real(p.1)))
    }

    /// Axes at `(x, y, z)` with the x axis along `dir`.
    fn axes(&mut self, (x, y, z): (f64, f64, f64), dir: Option<P2>) -> u64 {
        let o = self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(x), real(y), real(z)));
        match dir {
            Some(d) => {
                let d = self.add(format!("IFCDIRECTION(({},{},0.))", real(d.0), real(d.1)));
                self.add(format!("IFCAXIS2PLACEMENT3D(#{o},#{},#{d})", self.z))
            }
            None => self.add(format!("IFCAXIS2PLACEMENT3D(#{o},$,$)")),
        }
    }

    /// A placement at `xyz` with its x axis along `dir`, relative to `to` (none = the world).
    fn place(&mut self, to: Option<u64>, xyz: (f64, f64, f64), dir: Option<P2>) -> u64 {
        let a = self.axes(xyz, dir);
        let to = to.map_or_else(|| "$".to_string(), |t| format!("#{t}"));
        self.add(format!("IFCLOCALPLACEMENT({to},#{a})"))
    }

    /// A closed polyline through `ring`.
    fn polyline(&mut self, ring: &[P2]) -> u64 {
        let mut pts: Vec<u64> = ring.iter().map(|p| self.point(*p)).collect();
        pts.extend(pts.first().copied());
        self.add(format!("IFCPOLYLINE(({}))", refs(&pts)))
    }

    /// A rectangle `size` big centred on `centre`, as a profile.
    fn rectangle(&mut self, centre: P2, size: P2) -> u64 {
        let c = self.point(centre);
        let at = self.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
        self.add(format!("IFCRECTANGLEPROFILEDEF(.AREA.,$,#{at},{},{})", real(size.0), real(size.1)))
    }

    /// A profile around `outer`, less `holes`.
    fn profile(&mut self, outer: &[P2], holes: &[Vec<P2>]) -> u64 {
        let o = self.polyline(outer);
        if holes.is_empty() {
            return self.add(format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{o})"));
        }
        let inner: Vec<u64> = holes.iter().map(|h| self.polyline(h)).collect();
        self.add(format!("IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#{o},({}))", refs(&inner)))
    }

    /// A body: `profile` swept `depth` up from `bottom`, plus any extra representations.
    fn body(&mut self, profile: u64, bottom: f64, depth: f64, extra: &[u64]) -> u64 {
        self.bodies(&[(profile, bottom, depth)], extra)
    }

    /// A body of solids, each a profile swept `depth` up from `bottom`, plus any extra
    /// representations.
    fn bodies(&mut self, solids: &[(u64, f64, f64)], extra: &[u64]) -> u64 {
        let items: Vec<u64> = solids
            .iter()
            .map(|&(profile, bottom, depth)| {
                let at = if bottom == 0.0 { self.world } else { self.axes((0.0, 0.0, bottom), None) };
                self.add(format!("IFCEXTRUDEDAREASOLID(#{profile},#{at},#{},{})", self.z, real(depth)))
            })
            .collect();
        let rep = self.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','SweptSolid',({}))", self.body, refs(&items)));
        // Body first: readers that take the first representation get the solid.
        let reps: Vec<u64> = std::iter::once(rep).chain(extra.iter().copied()).collect();
        self.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,({}))", refs(&reps)))
    }

    /// A closed shell around the flat counter-clockwise `face` and its copy `rise` above, its
    /// faces turned out; heights from `base`.
    fn brep(&mut self, face: &[[f64; 3]], base: f64, rise: f64) -> u64 {
        let ring = |up: f64| -> Vec<[f64; 3]> { face.iter().map(|p| [p[0], p[1], p[2] - base + up]).collect() };
        self.shell(&ring(0.0), &ring(rise))
    }

    /// A closed shell from the counter-clockwise `bottom` ring up to `top`, the same points above
    /// it: its faces turned out.
    fn shell(&mut self, bottom: &[[f64; 3]], top: &[[f64; 3]]) -> u64 {
        let mut ring = |pts: &[[f64; 3]]| -> Vec<u64> {
            pts.iter().map(|p| self.add(format!("IFCCARTESIANPOINT(({},{},{}))", real(p[0]), real(p[1]), real(p[2])))).collect()
        };
        let (bottom, top) = (ring(bottom), ring(top));
        let mut loops = vec![bottom.iter().rev().copied().collect(), top.clone()];
        let pairs = || bottom.iter().zip(&top);
        loops.extend(pairs().zip(pairs().cycle().skip(1)).map(|((b0, t0), (b1, t1))| vec![*b0, *b1, *t1, *t0]));
        let faces: Vec<u64> = loops
            .iter()
            .map(|l| {
                let lp = self.add(format!("IFCPOLYLOOP(({}))", refs(l)));
                let bound = self.add(format!("IFCFACEOUTERBOUND(#{lp},.T.)"));
                self.add(format!("IFCFACE((#{bound}))"))
            })
            .collect();
        let shell = self.add(format!("IFCCLOSEDSHELL(({}))", refs(&faces)));
        self.add(format!("IFCFACETEDBREP(#{shell})"))
    }
}

pub fn export(m: &Model) -> String {
    let mut w = Step { out: String::new(), next: 0, uid: m.uid, world: 0, z: 0, body: 0 };
    let origin = w.add("IFCCARTESIANPOINT((0.,0.,0.))");
    w.z = w.add("IFCDIRECTION((0.,0.,1.))");
    w.world = w.add(format!("IFCAXIS2PLACEMENT3D(#{origin},$,$)"));
    let ctx = w.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{},$)", w.world));
    w.body = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{ctx},$,.MODEL_VIEW.,$)"));
    let axis = w.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,#{ctx},$,.GRAPH_VIEW.,$)"));
    let units = [
        "IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)",
        "IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)",
        "IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)",
        "IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)",
    ]
    .map(|u| w.add(u));
    let units = w.add(format!("IFCUNITASSIGNMENT(({}))", refs(&units)));
    let project = w.add(format!("IFCPROJECT({},$,{},$,$,$,$,(#{ctx}),#{units})", w.guid(PROJECT, 0), s(&m.name)));
    let site_at = w.place(None, (0.0, 0.0, 0.0), None);
    let site = w.add(format!("IFCSITE({},$,'Site',$,$,#{site_at},$,$,.ELEMENT.,$,$,$,$,$)", w.guid(SITE, 0)));
    let building_at = w.place(Some(site_at), (0.0, 0.0, 0.0), None);
    let building = w.add(format!("IFCBUILDING({},$,{},$,$,#{building_at},$,$,.ELEMENT.,$,$,$)", w.guid(BUILDING, 0), s(&m.name)));
    w.add(format!("IFCRELAGGREGATES({},$,$,$,#{project},(#{site}))", w.guid(AGGREGATES, PROJECT)));
    w.add(format!("IFCRELAGGREGATES({},$,$,$,#{site},(#{building}))", w.guid(AGGREGATES, SITE)));

    // Storeys: (entity, placement) by level.
    let mut storeys: BTreeMap<Id, (u64, u64)> = BTreeMap::new();
    for (id, l) in m.levels() {
        let at = w.place(Some(building_at), (0.0, 0.0, l.elevation), None);
        let st = w.add(format!("IFCBUILDINGSTOREY({},$,{},$,$,#{at},$,$,.ELEMENT.,{})", w.guid(STOREY, id), s(&l.name), real(l.elevation)));
        storeys.insert(id, (st, at));
    }
    let all: Vec<u64> = storeys.values().map(|(st, _)| *st).collect();
    if !all.is_empty() {
        w.add(format!("IFCRELAGGREGATES({},$,$,$,#{building},({}))", w.guid(AGGREGATES, BUILDING), refs(&all)));
    }

    let mut contained: BTreeMap<Id, Vec<u64>> = BTreeMap::new();
    let mut spaces: BTreeMap<Id, Vec<u64>> = BTreeMap::new();
    // Walls as (entity, placement), for the openings in them.
    let mut placed: BTreeMap<Id, (u64, u64)> = BTreeMap::new();
    for (&id, e) in &m.elements {
        match e {
            Element::Wall(wl) => {
                let Some(&(_, storey)) = storeys.get(&wl.level) else { continue };
                let (path, base) = (Axis::of(wl), m.own_base(wl));
                // Placed at its start, along the way it sets off: its axis, outline and openings in that frame.
                let u = path.tangent(0.0);
                let local = |p: P2| P2((p - wl.start).dot(u), (p - wl.start).dot(u.perp()));
                let at = w.place(Some(storey), (wl.start.0, wl.start.1, wl.base_offset), Some(u));
                let pts: Vec<u64> = path.points(0.0, path.length(), 0.0).into_iter().map(|p| w.point(local(p))).collect();
                let line = w.add(format!("IFCPOLYLINE(({}))", refs(&pts)));
                let axis_rep = w.add(format!("IFCSHAPEREPRESENTATION(#{axis},'Axis','Curve2D',(#{line}))"));
                // Its outline swept up, mitred where it joins others as the plan's is; attached to a
                // roof or a floor, a closed shell for each piece under (or over) a plane of it.
                let shape = if wl.attach_top.is_none() && wl.attach_base.is_none() {
                    let outline: Vec<P2> = walls::outline(m, id).into_iter().map(local).collect();
                    let profile = w.profile(&outline, &[]);
                    w.body(profile, 0.0, m.own_top(wl) - base, &[axis_rep])
                } else {
                    let breps: Vec<u64> = walls::prisms(m, id, false)
                        .iter()
                        .map(|p| {
                            let mut ring = p.ring.clone();
                            if geom::area(&ring) < 0.0 {
                                ring.reverse();
                            }
                            let up = |q: &P2, plane: &walls::Plane| {
                                let l = local(*q);
                                [l.0, l.1, plane.at(*q) - base]
                            };
                            let (bottom, top): (Vec<_>, Vec<_>) = ring.iter().map(|q| (up(q, &p.bottom), up(q, &p.top))).unzip();
                            w.shell(&bottom, &top)
                        })
                        .collect();
                    let rep = w.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Brep',({}))", w.body, refs(&breps)));
                    w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep},#{axis_rep}))"))
                };
                let ent = w.add(format!(
                    "IFCWALL({},$,{},$,{},#{at},#{shape},'{id}',.STANDARD.)",
                    w.guid(ELEMENT, id),
                    s(&wl.type_name),
                    s(&wl.type_name)
                ));
                contained.entry(wl.level).or_default().push(ent);
                placed.insert(id, (ent, at));
            }
            Element::Floor(f) => {
                let Some(&(_, storey)) = storeys.get(&f.level) else { continue };
                let t = m.types.floor.get(&f.type_name).map_or(0.0, |t| t.thickness);
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let profile = w.profile(&f.boundary, &[]);
                let shape = w.body(profile, -t, t, &[]);
                let ent = w.add(format!(
                    "IFCSLAB({},$,{},$,{},#{at},#{shape},'{id}',.FLOOR.)",
                    w.guid(ELEMENT, id),
                    s(&f.type_name),
                    s(&f.type_name)
                ));
                contained.entry(f.level).or_default().push(ent);
            }
            Element::Room(r) => {
                let Some(&(_, storey)) = storeys.get(&r.level) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                // A room that isn't enclosed has no shape to give.
                let shape = match m.room_region(r) {
                    Some(region) => {
                        let height = m.level_above(r.level).map_or(DEFAULT_HEIGHT, |up| m.elevation(up) - m.elevation(r.level));
                        let profile = w.profile(&region.outer, &region.holes);
                        format!("#{}", w.body(profile, 0.0, height, &[]))
                    }
                    None => "$".into(),
                };
                let ent = w.add(format!(
                    "IFCSPACE({},$,{},$,$,#{at},{shape},{},.ELEMENT.,.SPACE.,$)",
                    w.guid(ELEMENT, id),
                    s(&r.number),
                    s(&r.name)
                ));
                spaces.entry(r.level).or_default().push(ent);
            }
            Element::Column(col) => {
                let (Some(&(_, storey)), Some(t)) = (storeys.get(&col.level), m.types.column.get(&col.type_name)) else { continue };
                let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
                let at = w.place(Some(storey), (col.at.0, col.at.1, col.base_offset), Some(heading(col.angle)));
                let profile = if t.round {
                    let c = w.point(P2(0.0, 0.0));
                    let centre = w.add(format!("IFCAXIS2PLACEMENT2D(#{c},$)"));
                    w.add(format!("IFCCIRCLEPROFILEDEF(.AREA.,$,#{centre},{})", real(t.width / 2.0)))
                } else {
                    w.rectangle(P2(0.0, 0.0), P2(t.width, t.depth))
                };
                let shape = w.body(profile, 0.0, top - bottom, &[]);
                let ent = w.add(format!(
                    "IFCCOLUMN({},$,{},$,{},#{at},#{shape},'{id}',.COLUMN.)",
                    w.guid(ELEMENT, id),
                    s(&col.type_name),
                    s(&col.type_name)
                ));
                contained.entry(col.level).or_default().push(ent);
            }
            Element::Stair(st) => {
                let Some(&(_, storey)) = storeys.get(&st.level) else { continue };
                let (n, each, _) = m.flight(st);
                let at = w.place(Some(storey), (st.start.0, st.start.1, 0.0), Some(heading(st.angle)));
                // Each tread a block from the floor up to it, as in 3D.
                let steps: Vec<(u64, f64, f64)> = (1..n)
                    .map(|k| {
                        let k = f64::from(k);
                        (w.rectangle(P2((k - 0.5) * st.tread, 0.0), P2(st.tread, st.width)), 0.0, k * each)
                    })
                    .collect();
                let shape = w.bodies(&steps, &[]);
                let ent = w.add(format!("IFCSTAIR({},$,'Stair',$,$,#{at},#{shape},'{id}',.STRAIGHT_RUN_STAIR.)", w.guid(ELEMENT, id)));
                contained.entry(st.level).or_default().push(ent);
            }
            Element::Roof(r) => {
                let (Some(&(_, storey)), Ok((pieces, rise))) = (storeys.get(&r.level), m.roof_pieces(r)) else { continue };
                let at = w.place(Some(storey), (0.0, 0.0, 0.0), None);
                let breps: Vec<u64> = pieces.iter().map(|face| w.brep(face, m.elevation(r.level), rise)).collect();
                let rep = w.add(format!("IFCSHAPEREPRESENTATION(#{},'Body','Brep',({}))", w.body, refs(&breps)));
                let shape = w.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))"));
                let kind = match (r.slope == 0.0, r.gable) {
                    (true, _) => ".FLAT_ROOF.",
                    (false, true) => ".GABLE_ROOF.",
                    (false, false) => ".HIP_ROOF.",
                };
                let ent = w.add(format!(
                    "IFCROOF({},$,{},$,{},#{at},#{shape},'{id}',{kind})",
                    w.guid(ELEMENT, id),
                    s(&r.type_name),
                    s(&r.type_name)
                ));
                contained.entry(r.level).or_default().push(ent);
            }
            _ => {}
        }
    }
    for (&id, e) in &m.elements {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        let (Some(t), Some(wl), Some(&(wall, wall_at))) = (m.opening_type(e), m.wall(o.host), placed.get(&o.host)) else { continue };
        // From its jamb toward the wall's start, square to the wall at its middle, in the wall's frame.
        let axis = Axis::of(wl);
        let (u, way) = (axis.tangent(0.0), axis.tangent(o.offset));
        let jamb = axis.at(o.offset, walls::centre(m, wl)) - way * (t.width / 2.0) - wl.start;
        let at = w.place(Some(wall_at), (jamb.dot(u), jamb.dot(u.perp()), o.sill), Some(P2(way.dot(u), way.dot(u.perp()))));
        // The hole reaches 10 mm past both faces so it cuts cleanly; through a curved wall, past its bow too.
        let bow = axis.radius().map_or(0.0, |r| {
            let inner = r - m.thickness(wl) / 2.0 - walls::centre(m, wl).abs();
            if inner > t.width / 2.0 { inner - (inner * inner - t.width * t.width / 4.0).sqrt() } else { t.width }
        });
        let profile = w.rectangle(P2(t.width / 2.0, 0.0), P2(t.width, m.thickness(wl) + 20.0 + 2.0 * bow));
        let shape = w.body(profile, 0.0, t.height, &[]);
        let opening = w.add(format!("IFCOPENINGELEMENT({},$,'Opening',$,$,#{at},#{shape},$,.OPENING.)", w.guid(OPENING, id)));
        w.add(format!("IFCRELVOIDSELEMENT({},$,$,$,#{wall},#{opening})", w.guid(VOIDS, id)));
        // The door or window itself: a 50 mm panel centred in the wall.
        let fill_at = w.place(Some(at), (0.0, 0.0, 0.0), None);
        let profile = w.rectangle(P2(t.width / 2.0, 0.0), P2(t.width, 50.0));
        let shape = w.body(profile, 0.0, t.height, &[]);
        let (h, wd) = (real(t.height), real(t.width));
        let ent = if matches!(e, Element::Door(_)) {
            let swing = if o.flip_hand { ".SINGLE_SWING_RIGHT." } else { ".SINGLE_SWING_LEFT." };
            w.add(format!(
                "IFCDOOR({},$,{},$,{},#{fill_at},#{shape},{},{h},{wd},.DOOR.,{swing},$)",
                w.guid(ELEMENT, id),
                s(&o.type_name),
                s(&o.type_name),
                s(&o.mark)
            ))
        } else {
            w.add(format!(
                "IFCWINDOW({},$,{},$,{},#{fill_at},#{shape},{},{h},{wd},.WINDOW.,.SINGLE_PANEL.,$)",
                w.guid(ELEMENT, id),
                s(&o.type_name),
                s(&o.type_name),
                s(&o.mark)
            ))
        };
        w.add(format!("IFCRELFILLSELEMENT({},$,$,$,#{opening},#{ent})", w.guid(FILLS, id)));
        contained.entry(wl.level).or_default().push(ent);
    }
    // Compound wall types' layers: a layer set for each type, and for each type and line its walls
    // stand on, the set's use, from their exterior faces in.
    let mut materials: BTreeMap<&str, u64> = BTreeMap::new();
    let mut sets: BTreeMap<&str, u64> = BTreeMap::new();
    let used: std::collections::BTreeSet<&str> = placed.keys().filter_map(|id| m.wall(*id)).map(|wl| wl.type_name.as_str()).collect();
    for (name, t) in m.types.wall.iter().filter(|(name, t)| !t.layers.is_empty() && used.contains(name.as_str())) {
        let mut layers = Vec::new();
        for l in &t.layers {
            let material = match l.material.trim() {
                "" => "$".to_string(),
                named => {
                    let id = match materials.get(named) {
                        Some(id) => *id,
                        None => w.add(format!("IFCMATERIAL({},$,$)", s(named))),
                    };
                    materials.insert(named, id);
                    format!("#{id}")
                }
            };
            let category = match l.function {
                Function::Structure => "'LoadBearing'",
                Function::Thermal => "'Insulation'",
                Function::Finish => "'Finish'",
                Function::Substrate | Function::Membrane => "$",
            };
            layers.push(w.add(format!("IFCMATERIALLAYER({material},{},$,$,$,{category},$)", real(l.thickness))));
        }
        sets.insert(name, w.add(format!("IFCMATERIALLAYERSET(({}),{},$)", refs(&layers), s(name))));
    }
    let mut uses: BTreeMap<(&str, String), Vec<u64>> = BTreeMap::new();
    for (id, (ent, _)) in &placed {
        if let Some(wl) = m.wall(*id).filter(|wl| sets.contains_key(wl.type_name.as_str())) {
            uses.entry((wl.type_name.as_str(), real(walls::faces(m, wl).0))).or_default().push(*ent);
        }
    }
    for (k, ((name, offset), ents)) in uses.into_iter().enumerate() {
        let Some(set) = sets.get(name) else { continue };
        let usage = w.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS2.,.NEGATIVE.,{offset},$)"));
        w.add(format!("IFCRELASSOCIATESMATERIAL({},$,$,$,({}),#{usage})", w.guid(MATERIALS, k as u64), refs(&ents)));
    }
    for (level, (storey, _)) in &storeys {
        if let Some(items) = contained.get(level) {
            w.add(format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,({}),#{storey})", w.guid(CONTAINS, *level), refs(items)));
        }
        if let Some(items) = spaces.get(level) {
            w.add(format!("IFCRELAGGREGATES({},$,$,$,#{storey},({}))", w.guid(SPACES, *level), refs(items)));
        }
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let app = s(&format!("BIMCraft {}", env!("CARGO_PKG_VERSION")));
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition [DesignTransferView]'),'2;1');\n\
         FILE_NAME({},'{}',(''),(''),{app},{app},'');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
        s(&format!("{}.ifc", m.name)),
        utc(now),
        w.out
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use std::collections::BTreeSet;

    #[test]
    fn step_strings_escape_quotes_backslashes_and_other_scripts() {
        assert_eq!(s("it's"), "'it''s'");
        assert_eq!(s(r"a\b"), r"'a\\b'");
        assert_eq!(s("Área"), r"'\X2\00C1\X0\rea'");
        assert_eq!(s("x');#9=IFCWALL("), "'x'');#9=IFCWALL('", "a name can't end the string early");
    }

    #[test]
    fn reals_always_carry_a_decimal_point() {
        assert_eq!([real(1500.0), real(0.00001), real(-0.0), real(-200.5), real(f64::NAN)], ["1500.", "0.00001", "0.", "-200.5", "0."]);
    }

    #[test]
    fn global_ids_are_22_base64_characters_and_stable() {
        let a = guid(7, 5, 12);
        assert_eq!(a.len(), 22);
        assert!(matches!(a.as_bytes()[0], b'0'..=b'3'), "{a}");
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$'), "{a}");
        assert_eq!(a, guid(7, 5, 12));
        let all: BTreeSet<String> = (0..2000).flat_map(|id| [guid(7, 5, id), guid(7, 6, id), guid(8, 5, id)]).collect();
        assert_eq!(all.len(), 6000);
    }

    #[test]
    fn timestamps_are_utc_iso_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00");
        assert_eq!(utc(951_868_800), "2000-03-01T00:00:00");
        assert_eq!(utc(1_000_000_000), "2001-09-09T01:46:40");
    }

    fn sample_ifc() -> String {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        export(&s.model)
    }

    #[test]
    fn the_export_has_the_spatial_tree_and_every_element() {
        let f = sample_ifc();
        assert!(f.starts_with("ISO-10303-21;") && f.trim_end().ends_with("END-ISO-10303-21;"));
        assert!(f.contains("FILE_SCHEMA(('IFC4'));"));
        let n = |e: &str| f.matches(&format!("={e}(")).count();
        assert_eq!([n("IFCPROJECT"), n("IFCSITE"), n("IFCBUILDING"), n("IFCBUILDINGSTOREY")], [1, 1, 1, 2]);
        assert_eq!([n("IFCWALL"), n("IFCDOOR"), n("IFCWINDOW"), n("IFCSLAB"), n("IFCSPACE")], [7, 4, 5, 1, 4]);
        assert_eq!([n("IFCOPENINGELEMENT"), n("IFCRELVOIDSELEMENT"), n("IFCRELFILLSELEMENT")], [9, 9, 9]);
        assert!(f.contains("'Kitchen'"), "rooms keep their names");
    }

    #[test]
    fn compound_curved_and_attached_walls_export_their_layers_and_shapes() {
        let mut s = Session::new();
        s.script(
            r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "material": "Brick", "thickness": 100}, {"function": "structure", "material": "Block", "thickness": 200}]}
               wall {"points": [[0, 0], [0, 6000], [8000, 6000], [8000, 0]], "type": "Cavity"}
               wall {"points": [[8000, 0], [0, 0]], "arc": -60, "type": "Cavity", "location": "interior-finish"}
               window {"at": [4000, -1100]}
               roof {"gable": true, "boundary": [[-600, -1800], [8600, -1800], [8600, 6600], [-600, 6600]]}
               modify {"ids": [3, 5], "attach_top": 8}
               door {"at": [4000, 6000]}"#,
        )
        .unwrap();
        let f = export(&s.model);
        let n = |e: &str| f.matches(&format!("={e}(")).count();
        assert_eq!([n("IFCMATERIAL"), n("IFCMATERIALLAYER"), n("IFCMATERIALLAYERSET")], [2, 2, 1]);
        // One use for the walls on their centre lines, one for the curved one on its interior face.
        assert_eq!([n("IFCMATERIALLAYERSETUSAGE"), n("IFCRELASSOCIATESMATERIAL")], [2, 2]);
        assert!(
            f.contains("IFCMATERIALLAYERSETUSAGE(") && f.contains(".AXIS2.,.NEGATIVE.,150.,$)") && f.contains(".AXIS2.,.NEGATIVE.,300.,$)")
        );
        assert!(f.contains("IFCMATERIALLAYER(#") && f.contains("'LoadBearing'") && f.contains("'Brick'"));
        // The walls attached to the roof, and the roof, as closed shells; the others swept up.
        assert_eq!(f.matches("'Body','Brep'").count(), 3);
        assert_eq!([n("IFCWALL"), n("IFCOPENINGELEMENT"), n("IFCWINDOW")], [4, 2, 1]);
        // Every hole, through a straight wall or a curved one, has a size.
        for line in f.lines().filter(|l| l.contains("=IFCRECTANGLEPROFILEDEF(")) {
            assert!(!line.contains(",0.)") && !line.contains(",0.,"), "{line}");
        }
        dangling(&f);
    }

    /// Every reference in `f` points at an entity written once.
    fn dangling(f: &str) {
        let mut defined = BTreeSet::new();
        let mut referenced = BTreeSet::new();
        for line in f.lines().filter(|l| l.starts_with('#')) {
            let (head, body) = line.split_once('=').unwrap();
            assert!(defined.insert(head[1..].parse::<u64>().unwrap()), "{head} written twice");
            for r in body.split('#').skip(1) {
                referenced.insert(r.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap());
            }
        }
        let dangling: Vec<_> = referenced.difference(&defined).collect();
        assert!(dangling.is_empty(), "dangling references: {dangling:?}");
    }

    #[test]
    fn every_reference_points_at_an_entity_written_once() {
        let f = sample_ifc();
        let mut defined = BTreeSet::new();
        let mut referenced = BTreeSet::new();
        for line in f.lines().filter(|l| l.starts_with('#')) {
            let (head, body) = line.split_once('=').unwrap();
            assert!(defined.insert(head[1..].parse::<u64>().unwrap()), "{head} written twice");
            for r in body.split('#').skip(1) {
                referenced.insert(r.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap());
            }
        }
        let dangling: Vec<_> = referenced.difference(&defined).collect();
        assert!(dangling.is_empty(), "dangling references: {dangling:?}");
    }
}
