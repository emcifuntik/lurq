"""Build the lurq icon font, with its grid-fitting hints, from the SVG sources.

Reads:
  design/icons/svg/<name>.svg   one glyph each: a 24x24 viewBox of filled outlines, the SVG
                                export of the `Icon/<name>` component in pencil/lurq-design-system.pen
  design/icons/codepoints.json  name to codepoint in hex; a name keeps its codepoint for good

Writes:
  crates/lurq/assets/lurq-icons.ttf  the hinted font the toolkit embeds
  design/icons/codepoints.json       only to append a name it has not seen (from U+E000 up)

Run from the repository root (needs fontTools):

  python scripts/build-icon-font.py
  python scripts/build-icon-font.py --unhinted target/lurq-icons-unhinted.ttf

The second form also writes the same outlines without instructions, to compare against.
The build is deterministic: the same sources give the same bytes.

The hinter works on one axis at a time. It finds the edges that run straight across the
axis (and the extrema of wide arcs), and pairs a low edge with the high edge across the
black as a stem. The program it writes for a glyph then, at each size:

- gives a stem one pixel while it is thin, and from there the width it was drawn with,
  one edge on the pixel grid: the edge that faces out of the glyph;
- draws a stem in whole pixels instead where the white beside it is too narrow to keep
  a whole pixel next to an edge off the grid;
- places stems from the middle of the glyph outward, and moves one further out when the
  white between it and the stem before it would not keep a whole pixel.

Diagonal strokes and free stroke ends stay unhinted. `prep` asks for native ClearType:
without it a smooth rasterizer ignores hints along x.
"""

import argparse
import json
import xml.etree.ElementTree as ET
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path
from fontTools.ttLib import newTable
from fontTools.ttLib.tables._g_l_y_f import flagOverlapSimple
from fontTools.ttLib.tables.ttProgram import Program


ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / "design" / "icons" / "svg"
CODEPOINTS = ROOT / "design" / "icons" / "codepoints.json"
FONT = ROOT / "crates" / "lurq" / "assets" / "lurq-icons.ttf"

UPM = 2400
UNIT = 100  # font units per design unit: the icons are drawn on a 24-unit grid
FIRST_CODEPOINT = 0xE000
FAMILY = "lurq icons"
EPOCH = 3_850_070_400  # the font's date: 1 January 2026, in seconds since 1904
SVG ="{http://www.w3.org/2000/svg}"

# In font units.
MIN_EDGE = 80  # shortest straight run that counts as an edge
MIN_ARC = 80  # an extremum counts only on an arc this wide, which leaves out round caps
MAX_DOT = 400  # a contour no larger than this is a dot: its four extremes are edges
MAX_STEM = 400  # widest pair of edges treated as one stem
MAX_GAP = 500  # widest white between two edges that is watched for closing
MAX_ROOM = 400  # widest white that can leave a stem short of room
LINK = 150  # a lone edge this close to a placed one moves with it
FOLLOW = 110  # a stroke end this close to an edge it runs into moves with that edge
TOL = 2
PROBE = 12  # how far off an edge its two sides are sampled for black

# In 64ths of a pixel.
THIN = 92  # a stem narrower than this is drawn one pixel wide
SNAP = 6  # an edge this close to a pixel boundary counts as on it
FAR = 16384  # stands for no white beside a stem

# Storage slots and functions of the font program.
S_LOW, S_HIGH, S_NATURAL, S_WIDTH, S_WHITE, S_MIDDLE = range(1, 7)
F_WIDTH, F_WHITE, F_MIDDLE, F_STEM, F_STEM_MIDDLE, F_STEM_UP, F_HIGH, F_LOW, F_GAP, F_GAP_DOWN = range(10)
# A distance as the outline was drawn, whatever has moved since. Interpreters read the flag of
# MD the other way round from the specification: 1 is the original outline.
MEASURE = "MD[1]"


def push(*values):
    return ["PUSH[ ] " + " ".join(map(str, values))]


def font_program():
    """The functions the glyph programs call, as assembly, and how many there are."""
    store = lambda slot: push(slot) + ["SWAP[ ]", "WS[ ]"]
    load = lambda slot: push(slot) + ["RS[ ]"]
    # ( low high -- ) with both points remembered and the stem's two widths worked out.
    measure = (
        store(S_HIGH) + store(S_LOW)
        + load(S_HIGH) + load(S_LOW) + [MEASURE, "DUP[ ]"] + store(S_NATURAL)
        + push(F_WIDTH) + ["CALL[ ]"] + store(S_WIDTH)
    )
    # Half of what the stem loses against its natural width: its centre stays where it was drawn.
    inset = load(S_NATURAL) + load(S_WIDTH) + ["SUB[ ]"] + push(32) + ["MUL[ ]"]
    # ( low high -- white ) The whole pixels of white between a high edge and the low edge above it.
    white = (
        ["GC[0]"] + push(SNAP) + ["SUB[ ]", "CEILING[ ]", "SWAP[ ]", "GC[0]"] + push(SNAP) + ["ADD[ ]", "FLOOR[ ]"]
        + ["SWAP[ ]", "SUB[ ]"]
    )
    # ( white -- shift ) What the white lacks of one pixel, and whether it lacks any: the IF is left open.
    lacking = push(64) + ["SWAP[ ]", "SUB[ ]", "DUP[ ]"] + push(0) + ["GT[ ]", "IF[ ]"]
    # ( white -- ) Move the stem in S_LOW and S_HIGH by what the white lacks of one pixel.
    move_stem = lambda sign: (
        lacking + sign + ["DUP[ ]"] + load(S_LOW) + ["SWAP[ ]", "SHPIX[ ]"] + load(S_HIGH) + ["SWAP[ ]", "SHPIX[ ]"]
        + ["ELSE[ ]", "POP[ ]", "EIF[ ]"]
    )
    # ( low high -- ) Keep the white between two edges in a slot if it is the narrowest noted there.
    narrowest = lambda slot: [MEASURE] + load(slot) + ["MIN[ ]"] + store(slot)
    # ( width -- room ) What a stem with an edge off the grid needs along the axis: the pixels it
    # touches, and a whole one of white next to them. For one stem, or for the two across the middle.
    room = lambda stems: push(SNAP) + ["SUB[ ]", "CEILING[ ]"] + ["DUP[ ]", "ADD[ ]"] * (stems - 1) + push(64 - 2) + ["ADD[ ]"]
    # The stem in S_LOW and S_HIGH goes where its low edge is on the grid.
    by_low = (
        load(S_LOW) + ["DUP[ ]", "GC[0]"] + inset + ["ADD[ ]"] + push(1) + ["SUB[ ]", "ROUND[00]", "SCFS[ ]"]
        + load(S_LOW) + ["SRP0[ ]"] + load(S_HIGH) + load(S_WIDTH) + ["MSIRP[0]"]
    )
    functions = {
        # ( natural -- width ) One pixel for a thin stem; from there on the width it was drawn with,
        # so a straight stroke weighs what the diagonal beside it does. That leaves one edge of the
        # stem off the grid, and next to the pixel that edge ends in the white still needs a whole
        # one. Where the white beside the stem is too narrow for that, the stem is drawn in whole
        # pixels, rounded down. Across the middle of the glyph two stems turn their soft edges to
        # each other, and the white has to do for both.
        F_WIDTH: ["DUP[ ]"] + push(THIN) + ["LT[ ]", "IF[ ]", "POP[ ]"] + push(64) + ["ELSE[ ]"]
        + ["DUP[ ]"] + room(1) + load(S_NATURAL) + load(S_WHITE) + ["ADD[ ]", "GT[ ]"]
        + load(S_NATURAL) + room(2) + load(S_NATURAL) + ["DUP[ ]", "ADD[ ]"] + load(S_MIDDLE) + ["ADD[ ]", "GT[ ]"]
        + ["OR[ ]", "IF[ ]"] + push(SNAP) + ["ADD[ ]", "FLOOR[ ]", "EIF[ ]"]
        + ["EIF[ ]"]
        + push(S_WHITE, FAR) + ["WS[ ]"] + push(S_MIDDLE, FAR) + ["WS[ ]"],
        # ( low high -- ) Note the white between a high edge and the low edge above it, one of them
        # an edge of the stem about to be placed.
        F_WHITE: narrowest(S_WHITE),
        # ( low high -- ) The same for the white across the middle of the glyph.
        F_MIDDLE: narrowest(S_MIDDLE),
        # ( low high -- ) Place a stem with its low edge on the grid. Halfway between two pixels it
        # goes down, where the mirrored stem placed by its high edge goes up: a symmetric glyph stays so.
        F_STEM: measure + by_low,
        # ( low high -- ) Place the stem that sits on the middle of the glyph. From two pixels up, at
        # the width it was drawn with, it stays where it was drawn: an edge on the grid would put it
        # off the middle, and the row of dots it is one of would be unevenly spaced.
        F_STEM_MIDDLE: measure
        + load(S_WIDTH) + push(128) + ["GTEQ[ ]"] + load(S_WIDTH) + load(S_NATURAL) + ["EQ[ ]", "AND[ ]", "NOT[ ]", "IF[ ]"]
        + by_low + ["EIF[ ]"],
        # ( low high -- ) Place a stem with its high edge on the grid.
        F_STEM_UP: measure
        + load(S_HIGH) + ["DUP[ ]", "GC[0]"] + inset + ["SUB[ ]", "ROUND[00]", "SCFS[ ]"]
        + load(S_HIGH) + ["SRP0[ ]"] + load(S_LOW) + load(S_WIDTH) + ["NEG[ ]", "MSIRP[0]"],
        # ( low high -- ) The low edge is placed: put the high edge the stem's width above it.
        F_HIGH: measure + load(S_LOW) + ["SRP0[ ]"] + load(S_HIGH) + load(S_WIDTH) + ["MSIRP[0]"],
        # ( low high -- ) The high edge is placed: put the low edge the stem's width below it.
        F_LOW: measure + load(S_HIGH) + ["SRP0[ ]"] + load(S_LOW) + load(S_WIDTH) + ["NEG[ ]", "MSIRP[0]"],
        # ( below low high -- ) Push a stem up until a whole pixel of white lies between it and the edge below.
        F_GAP: store(S_HIGH) + store(S_LOW) + load(S_LOW) + ["SWAP[ ]"] + white + move_stem([]),
        # ( above low high -- ) Push a stem down until a whole pixel of white lies between it and the edge above.
        F_GAP_DOWN: store(S_HIGH) + store(S_LOW) + load(S_HIGH) + white + move_stem(["NEG[ ]"]),
    }
    lines = []
    for number, body in sorted(functions.items()):
        lines += push(number) + ["FDEF[ ]"] + body + ["ENDF[ ]"]
    return lines, len(functions)


def split_at_extrema(start, control, end):
    """A quadratic curve as one or more, so that each of its extrema is an on-curve point."""
    cuts = set()
    for axis in (0, 1):
        bend = start[axis] - 2 * control[axis] + end[axis]
        if bend and 0.02 < (start[axis] - control[axis]) / bend < 0.98:
            cuts.add((start[axis] - control[axis]) / bend)
    pieces, done = [], 0.0
    for cut in sorted(cuts) + [1.0]:
        # The part of the remaining curve up to `cut`, by de Casteljau.
        t = (cut - done) / (1 - done)
        first = tuple(a + (b - a) * t for a, b in zip(start, control))
        second = tuple(a + (b - a) * t for a, b in zip(control, end))
        middle = tuple(a + (b - a) * t for a, b in zip(first, second))
        if cut == 1.0:
            pieces.append((control, end))
        else:
            pieces.append((first, middle))
            start, control, done = middle, second, cut
    return pieces


def read_contours(path):
    """The glyph's contours as pen operations, in whole font units with y up.

    Curves are cut at their extrema: an extremum that is an on-curve point can be hinted.
    """
    recording = RecordingPen()
    pen = TransformPen(recording, (UNIT, 0, 0, -UNIT, 0, UPM))
    for element in ET.parse(path).getroot().iter(SVG + "path"):
        parse_path(element.get("d"), pen)
    whole = lambda point: (round(point[0]), round(point[1]))
    contours, current, here = [], [], None

    def line(end):
        if end != current[-1][1][-1]:
            current.append(("lineTo", (end,)))

    for operation, arguments in recording.value:
        if operation == "qCurveTo":
            if len(arguments) != 2:
                raise SystemExit(f"{path.name}: expected simple quadratic curves")
            for control, end in split_at_extrema(here, *arguments):
                # A curve too small to survive rounding is a line, or nothing: no point comes twice.
                if whole(control) in (current[-1][1][-1], whole(end)):
                    line(whole(end))
                else:
                    current.append((operation, (whole(control), whole(end))))
        elif operation == "lineTo":
            line(whole(arguments[0]))
        elif operation == "moveTo":
            current.append((operation, (whole(arguments[0]),)))
        elif operation in ("closePath", "endPath"):
            current.append(("closePath", ()))
            if len(current) > 3:
                contours.append(current)
            current = []
        else:
            raise SystemExit(f"{path.name}: unexpected {operation} in the outline")
        if arguments:
            here = arguments[-1]
    return contours


def make_glyph(contours):
    pen = TTGlyphPen(None)
    for contour in contours:
        for operation, arguments in contour:
            getattr(pen, operation)(*arguments)
    glyph = pen.glyph()
    if glyph.numberOfContours > 0:
        # The pieces of a stroke are separate contours that overlap.
        glyph.flags[0] |= flagOverlapSimple
    return glyph


def flatten(contours):
    """The contours as closed polylines, for telling black from white."""
    polylines = []
    for contour in contours:
        line = []
        for operation, arguments in contour:
            if operation in ("moveTo", "lineTo"):
                line.append(arguments[0])
            elif operation == "qCurveTo":
                (x0, y0), (cx, cy), (x1, y1) = line[-1], arguments[0], arguments[1]
                for step in range(1, 9):
                    t = step / 8
                    line.append(((1 - t) ** 2 * x0 + 2 * t * (1 - t) * cx + t * t * x1, (1 - t) ** 2 * y0 + 2 * t * (1 - t) * cy + t * t * y1))
        polylines.append(line)
    return polylines


def is_black(polylines, x, y):
    winding = 0
    for line in polylines:
        for k in range(len(line)):
            (x0, y0), (x1, y1) = line[k], line[(k + 1) % len(line)]
            if (y0 <= y) != (y1 <= y) and x0 + (y - y0) * (x1 - x0) / (y1 - y0) > x:
                winding += 1 if y1 > y0 else -1
    return winding != 0


def contour_points(glyph):
    start, contours = 0, []
    for end in glyph.endPtsOfContours:
        contours.append(list(range(start, end + 1)))
        start = end + 1
    return contours


def find_edges(glyph, polylines, axis):
    """Edges that lie across `axis`: straight runs, the extrema of wide arcs, the extremes of dots.

    An edge is `low` when the black is on its higher side. An edge buried under another
    contour has black on both sides and is left out. Also returns the stroke ends: the
    extrema of arcs too narrow to be edges, as (coordinate, point, position along the edge).
    """
    other = 1 - axis
    xy, on_curve = glyph.coordinates, [flag & 1 for flag in glyph.flags]

    def side(at, along):
        """True or False for a low or high edge at this spot, None where it is buried or stray."""
        spot = lambda offset: (at + offset, along) if axis == 0 else (along, at + offset)
        below, above = is_black(polylines, *spot(-PROBE)), is_black(polylines, *spot(PROBE))
        return None if below == above else above

    edges, ends = [], []

    def add(at, points, lo, hi, samples):
        sides = [low for low in (side(at, along) for along in samples) if low is not None]
        if sides:
            edges.append({"at": at, "low": sum(sides) * 2 >= len(sides), "points": points, "span": (lo, hi)})

    for indices in contour_points(glyph):
        count = len(indices)
        across = [xy[i][axis] for i in indices]
        along = [xy[i][other] for i in indices]
        if max(across) - min(across) <= MAX_DOT and max(along) - min(along) <= MAX_DOT:
            middle = (min(along) + max(along)) / 2
            for extreme in (min(across), max(across)):
                points = [i for i in indices if on_curve[i] and abs(xy[i][axis] - extreme) <= TOL]
                if points:
                    add(extreme, points, min(along), max(along), [middle])
            continue
        for k in range(count):
            i, after, before = indices[k], indices[(k + 1) % count], indices[(k - 1) % count]
            at = xy[i][axis]
            if on_curve[i] and on_curve[after] and xy[after][axis] == at and abs(xy[after][other] - xy[i][other]) >= MIN_EDGE:
                lo, hi = sorted((xy[i][other], xy[after][other]))
                steps = max(3, int((hi - lo) // 25))
                add(at, [i, after], lo, hi, [lo + (hi - lo) * (s + 0.5) / steps for s in range(steps)])
            elif on_curve[i] and not on_curve[after] and not on_curve[before]:
                flat = abs(xy[before][axis] - at) <= TOL and abs(xy[after][axis] - at) <= TOL
                if flat and abs(xy[after][other] - xy[before][other]) >= MIN_ARC:
                    lo, hi = sorted((xy[before][other], xy[after][other]))
                    add(at, [i], lo, hi, [xy[i][other]])
                elif flat:
                    ends.append((at, i, xy[i][other]))
    return edges, ends


def group_edges(edges):
    """Edges at one coordinate that face the same way move as one."""
    groups = []
    for edge in sorted(edges, key=lambda edge: (edge["low"], edge["at"])):
        last = groups[-1] if groups else None
        if last and last["low"] == edge["low"] and abs(last["at"] - edge["at"]) <= TOL:
            last["points"].extend(point for point in edge["points"] if point not in last["points"])
            last["spans"].append(edge["span"])
        else:
            groups.append({"at": edge["at"], "low": edge["low"], "points": list(edge["points"]), "spans": [edge["span"]]})
    return groups


def shared_span(first, second):
    """The middle of the longest stretch along which two groups of edges face each other."""
    best = None
    for a0, a1 in first["spans"]:
        for b0, b1 in second["spans"]:
            lo, hi = max(a0, b0), min(a1, b1)
            if hi > lo and (best is None or hi - lo > best[1] - best[0]):
                best = (lo, hi)
    return None if best is None else (best[0] + best[1]) / 2


def between(polylines, axis, first, second, black):
    """Whether two groups of edges face each other across black (or across white)."""
    along = shared_span(first, second)
    if along is None:
        return False
    at = (first["at"] + second["at"]) / 2
    return is_black(polylines, *((at, along) if axis == 0 else (along, at))) == black


def unit_edges(unit):
    return [unit[key] for key in ("low", "high") if key in unit]


def is_upper(unit):
    """Whether a stem or a lone edge lies in the upper half of the glyph along the axis.

    The two halves are hinted outward from the middle, each the mirror image of the other,
    so a symmetric glyph stays symmetric. A stem keeps the edge that faces out on the grid:
    in the upper half its high edge, in the lower half its low one.
    """
    edges = unit_edges(unit)
    return sum(edge["at"] for edge in edges) / len(edges) > UPM / 2


def analyse_axis(glyph, polylines, axis):
    """The stems and lone edges of a glyph along one axis, lowest first, and its stroke ends.

    A unit is a stem (a low and a high edge across black) or a lone edge. Each gets
    `below`: the high edges under it that face its low edge across white; `above`: the low
    edges over it that face its high edge; and, for a stem, `whites`: the narrow whites on
    either side, as (low point, high point, across the middle). The stroke ends that run into an edge
    come as (edge, point): they move with it.
    """
    edges, ends = find_edges(glyph, polylines, axis)
    groups = group_edges(edges)
    lows = sorted((g for g in groups if g["low"]), key=lambda g: g["at"])
    highs = sorted((g for g in groups if not g["low"]), key=lambda g: g["at"])
    across = lambda low, high: 0 < high["at"] - low["at"] <= MAX_STEM and between(polylines, axis, low, high, black=True)
    units = []
    for low in lows:
        # A stem is a low edge and the nearest high edge across black, each the nearest to the other.
        high = next((high for high in highs if across(low, high)), None)
        if high is not None and next(other for other in reversed(lows) if across(other, high)) is low:
            units.append({"low": low, "high": high})
    paired = {id(group) for unit in units for group in unit_edges(unit)}
    units += [{"low" if g["low"] else "high": g} for g in groups if id(g) not in paired]
    units.sort(key=lambda unit: min(g["at"] for g in unit_edges(unit)))
    for unit in units:
        unit["below"], unit["above"], unit["whites"] = [], [], []
    for index, unit in enumerate(units):
        if "low" not in unit:
            continue
        for earlier in units[:index]:
            if "high" not in earlier:
                continue
            white = unit["low"]["at"] - earlier["high"]["at"]
            if 0 < white <= MAX_GAP and between(polylines, axis, earlier["high"], unit["low"], black=False):
                unit["below"].append(earlier["high"])
                earlier["above"].append(unit["low"])
                if white <= MAX_ROOM and "low" in earlier and "high" in unit:
                    middle = not is_upper(earlier) and is_upper(unit)
                    for stem in (unit, earlier):
                        stem["whites"].append((unit["low"]["points"][0], earlier["high"]["points"][0], middle))
    followers = []
    for at, point, along in ends:
        reach = lambda g: abs(g["at"] - at) <= FOLLOW and any(lo - FOLLOW <= along <= hi + FOLLOW for lo, hi in g["spans"])
        edge = min((g for g in groups if reach(g)), key=lambda g: abs(g["at"] - at), default=None)
        if edge is not None:
            followers.append((edge, point))
    return units, followers


def axis_program(glyph, polylines, axis):
    units, followers = analyse_axis(glyph, polylines, axis)
    if not units:
        return []
    lines, placed = ["SVTCA[%d]" % (1 - axis)], []
    is_placed = lambda group: any(group is other for other in placed)
    call = lambda function, *points: push(*points, function) + ["CALL[ ]"]
    follow = lambda edge, point: push(edge["points"][0]) + ["SRP0[ ]"] + push(point) + ["MDRP[00000]"]

    def place_edge(unit):
        """A lone edge is half a stroke whose other half was not found: on the grid alone it would
        change the weight of the stroke. It moves only with a placed edge right beside it."""
        edge = unit_edges(unit)[0]
        near = min(placed, key=lambda other: abs(other["at"] - edge["at"]), default=None)
        if near is not None and abs(near["at"] - edge["at"]) <= LINK:
            return follow(near, edge["points"][0])
        return []

    def place_stem(unit, upper):
        low, high = unit["low"], unit["high"]
        pair = (low["points"][0], high["points"][0])
        lines = []
        for above, below, middle in unit["whites"]:
            lines += call(F_MIDDLE if middle else F_WHITE, above, below)
        if is_placed(low):
            return lines + call(F_HIGH, *pair)
        if is_placed(high):
            return lines + call(F_LOW, *pair)
        on_middle = abs(low["at"] + high["at"] - UPM) <= TOL
        lines += call(F_STEM_UP if upper else F_STEM_MIDDLE if on_middle else F_STEM, *pair)
        for other in unit["below"] if upper else unit["above"]:
            if is_placed(other):
                lines += call(F_GAP if upper else F_GAP_DOWN, other["points"][0], *pair)
        return lines

    # Stems from the middle outward: the lower half downward, then the upper half upward. Then
    # what moves along with them: lone edges, and the stroke ends that run into an edge.
    stems = [unit for unit in units if len(unit_edges(unit)) == 2]
    lone = [unit for unit in units if len(unit_edges(unit)) == 1]
    for unit in [unit for unit in reversed(stems) if not is_upper(unit)] + [unit for unit in stems if is_upper(unit)] + lone:
        todo = [group for group in unit_edges(unit) if not is_placed(group)]
        moved = todo and (place_stem(unit, is_upper(unit)) if unit in stems else place_edge(unit))
        if not moved:
            continue
        lines += moved
        for group in todo:
            rest = group["points"][1:]
            if rest:
                lines += push(group["points"][0]) + ["SRP0[ ]"] + push(*rest, len(rest)) + ["SLOOP[ ]", "ALIGNRP[ ]"]
        placed += todo
    for edge, point in followers:
        if is_placed(edge):
            lines += follow(edge, point)
    return lines if placed else []


def hint(glyph, contours):
    """Give the glyph its program. Returns how deep the program's pushes go."""
    if glyph.numberOfContours <= 0:
        return 0
    polylines = flatten(contours)
    lines = axis_program(glyph, polylines, 0) + axis_program(glyph, polylines, 1)
    if not lines:
        return 0
    glyph.program = Program()
    glyph.program.fromAssembly("\n".join(lines + ["IUP[1]", "IUP[0]"]))
    return max(len(line.split()) - 2 for line in lines if line.startswith("PUSH"))


def read_codepoints(names):
    """Codepoints by name. A name seen before keeps its codepoint; a new one takes the next free."""
    known = json.loads(CODEPOINTS.read_text(encoding="utf-8")) if CODEPOINTS.is_file() else {}
    codepoints = {name: int(value, 16) for name, value in known.items()}
    for name in names:
        if name not in codepoints:
            codepoints[name] = max(codepoints.values(), default=FIRST_CODEPOINT - 1) + 1
    if len(codepoints) != len(known):
        text = json.dumps({name: "%04X" % value for name, value in codepoints.items()}, indent=2)
        CODEPOINTS.write_text(text + "\n", encoding="utf-8", newline="\n")
    return codepoints


def build(sources, codepoints, hinted):
    names = sorted(sources, key=lambda name: codepoints[name])
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    deepest = 0
    for name in names:
        glyphs[name] = make_glyph(sources[name])
        if hinted:
            deepest = max(deepest, hint(glyphs[name], sources[name]))
    order = [".notdef"] + names
    builder = FontBuilder(UPM, isTTF=True)
    builder.setupGlyphOrder(order)
    builder.setupCharacterMap({codepoints[name]: name for name in names})
    builder.setupGlyf(glyphs)
    glyf = builder.font["glyf"]
    builder.setupHorizontalMetrics({name: (UPM, getattr(glyf[name], "xMin", 0)) for name in order})
    builder.setupHorizontalHeader(ascent=UPM, descent=0)
    # A font database takes a face only with a family and a PostScript name.
    builder.setupNameTable({"familyName": FAMILY, "styleName": "Regular", "fullName": FAMILY, "psName": "lurq-icons"})
    builder.setupOS2(sTypoAscender=UPM, sTypoDescender=0, usWinAscent=UPM, usWinDescent=0)
    builder.setupPost(keepGlyphNames=False)
    # No clock in the file: the same sources give the same bytes.
    builder.font["head"].created = builder.font["head"].modified = EPOCH
    builder.font.recalcTimestamp = False
    if hinted:
        program, functions = font_program()
        fpgm = builder.font["fpgm"] = newTable("fpgm")
        fpgm.program = Program()
        fpgm.program.fromAssembly("\n".join(program))
        # Ask for native ClearType, so a smooth rasterizer honours hints along x too; and start
        # every glyph with no white recorded beside a stem.
        prep = builder.font["prep"] = newTable("prep")
        prep.program = Program()
        prep.program.fromAssembly("\n".join(push(4, 3) + ["INSTCTRL[ ]"] + push(S_WHITE, FAR) + ["WS[ ]"] + push(S_MIDDLE, FAR) + ["WS[ ]"]))
        maxp = builder.font["maxp"]
        maxp.maxZones = 1
        maxp.maxFunctionDefs = functions
        maxp.maxStorage = S_MIDDLE + 1
        maxp.maxStackElements = deepest + 16
        maxp.maxSizeOfInstructions = max(len(glyph.program.getBytecode()) for glyph in glyphs.values() if hasattr(glyph, "program"))
        # Grid-fit and smooth at every size.
        gasp = builder.font["gasp"] = newTable("gasp")
        gasp.version = 1
        gasp.gaspRange = {0xFFFF: 0x000F}
    return builder.font


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--unhinted", type=Path, help="also write the font without hints to this path, to compare against")
    args = parser.parse_args()

    sources = {path.stem: read_contours(path) for path in sorted(SOURCES.glob("*.svg"))}
    if not sources:
        raise SystemExit(f"No glyphs in {SOURCES}")
    codepoints = read_codepoints(sorted(sources))

    FONT.parent.mkdir(parents=True, exist_ok=True)
    build(sources, codepoints, hinted=True).save(FONT)
    print(f"Saved {FONT.relative_to(ROOT).as_posix()}: {len(sources)} glyphs, {FONT.stat().st_size} bytes.")
    if args.unhinted:
        args.unhinted.parent.mkdir(parents=True, exist_ok=True)
        build(sources, codepoints, hinted=False).save(args.unhinted)
        print(f"Saved {args.unhinted}: the same outlines without hints.")


if __name__ == "__main__":
    main()
