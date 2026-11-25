#!/usr/bin/env python3
"""CayenneLPP readings as MeshCore v1.18 nodes send them (protocol.md §16.7, ADR-0050).

An independent oracle, written from the library the firmware builds with
(electroniccats/CayenneLPP 1.6.1, src/CayenneLPP.{h,cpp}): `addField` multiplies a float by
the type's multiplier, truncates toward zero, writes the magnitude in two's complement for the
signed types and big-endian; `addGPS` writes three 24-bit fields. Nothing is shared with the Rust
decoder. Each vector is the bytes, how many of them hold known items, and those items in the units
lichen keeps (millivolts, tenths of a degree, ...). Regenerate with:
    python3 -I testdata/lichen/lpp.py > testdata/lichen/lpp.toml
"""
import struct

# type: (name, size, multiplier, signed) -- CayenneLPP.h L27-L110, getTypeSigned (cpp L556)
TYPES = {
    2: ("analog_input", 2, 100, True),
    100: ("generic", 4, 1, False),
    101: ("illuminance", 2, 1, False),
    103: ("temperature", 2, 10, True),
    104: ("humidity", 1, 2, False),
    115: ("pressure", 2, 10, False),
    116: ("voltage", 2, 100, True),
    117: ("current", 2, 1000, True),
    118: ("frequency", 4, 1, False),
    120: ("percentage", 1, 1, False),
    121: ("altitude", 2, 1, True),
    128: ("power", 2, 1, False),
    130: ("distance", 4, 1000, False),
}
GPS = 136
BY_NAME = {v[0]: k for k, v in TYPES.items()}

# From the raw integer to the unit lichen keeps.
KEPT = {
    "analog_input": lambda r: r,  # hundredths
    "generic": lambda r: r,
    "illuminance": lambda r: r,  # lux
    "temperature": lambda r: r,  # tenths of a degree
    "humidity": lambda r: r * 5,  # tenths of a percent (raw is half percents)
    "pressure": lambda r: r,  # tenths of a hPa
    "voltage": lambda r: r * 10,  # millivolts (raw is hundredths of a volt)
    "current": lambda r: r,  # milliamps
    "frequency": lambda r: r,  # hertz
    "percentage": lambda r: r,
    "altitude": lambda r: r,  # metres
    "power": lambda r: r,  # watts
    "distance": lambda r: r,  # millimetres
}


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def add_field(name, channel, value):
    """addField: float value, uint32 multiplier, MSB first. Returns (bytes, raw signed int)."""
    t = BY_NAME[name]
    _, size, mult, signed = TYPES[t]
    v = f32(value)
    sign = v < 0
    if sign:
        v = -v
    u = int(f32(v * mult))  # truncation toward zero, as the C cast
    mask = (1 << (size * 8)) - 1
    if signed and sign:
        u = (mask - (u & mask) + 1) & mask
    u &= mask
    raw = u - (1 << (size * 8)) if signed and u >> (size * 8 - 1) else u
    return bytes([channel, t]) + u.to_bytes(size, "big"), raw


def add_gps(channel, lat, lon, alt):
    """addGPS: int32 casts of the products, the low 24 bits of each, MSB first."""
    vals = [int(f32(f32(lat) * 10000)), int(f32(f32(lon) * 10000)), int(f32(f32(alt) * 100))]
    out = bytes([channel, GPS])
    raws = []
    for v in vals:
        u = v & 0xFFFFFF
        out += u.to_bytes(3, "big")
        raws.append(u - (1 << 24) if u >> 23 else u)
    return out, raws


def item(name, channel, value):
    b, raw = add_field(name, channel, value)
    return b, {"channel": channel, "kind": name, "value": KEPT[name](raw)}


def gps(channel, lat, lon, alt):
    b, (la, lo, al) = add_gps(channel, lat, lon, alt)
    return b, {"channel": channel, "kind": "gps", "lat": la, "lon": lo, "alt": al}


def vector(name, parts, tail=b""):
    data = b"".join(p[0] for p in parts) + tail
    consumed = sum(len(p[0]) for p in parts)
    return {"name": name, "hex": data.hex(), "consumed": consumed, "items": [p[1] for p in parts]}


def vectors():
    v = []
    v.append(vector("a repeater's base readings", [item("voltage", 1, 3.92), item("temperature", 1, 4.0)]))
    v.append(vector("a cold night", [item("voltage", 1, 3.55), item("temperature", 1, -12.5)]))
    v.append(
        vector(
            "a node with its position and sensors",
            [
                item("voltage", 1, 4.05),
                gps(1, 12.3456, -45.6789, 250.25),
                item("temperature", 1, 21.3),
                item("temperature", 2, -7.5),
                item("humidity", 2, 64.5),
                item("pressure", 2, 845.2),
                item("altitude", 2, 1520),
                item("voltage", 3, 12.6),
                item("current", 3, 0.125),
                item("power", 3, 3),
            ],
        )
    )
    v.append(
        vector(
            "the other types",
            [
                item("analog_input", 2, -1.23),
                item("illuminance", 3, 520),
                item("percentage", 4, 87),
                item("frequency", 5, 868000000),
                item("distance", 6, 12.345),
                item("generic", 7, 123456),
            ],
        )
    )
    v.append(vector("a position south and west", [gps(1, -33.8, -70.5, -12.5)]))
    v.append(vector("the extremes", [item("temperature", 1, -3276.8), item("voltage", 1, 327.67), item("humidity", 2, 127.5)]))
    v.append(vector("empty", []))
    # Unknown types stop the decoding; what came before stays.
    v.append(vector("an unknown type", [item("voltage", 1, 3.92)], bytes([1, 200, 0, 0]) + item("temperature", 1, 4.0)[0]))
    v.append(vector("a type outside the subset (accelerometer)", [item("voltage", 1, 3.7)], bytes([2, 113, 0, 1, 0, 2, 0, 3])))
    v.append(vector("a polyline (variable length)", [item("percentage", 1, 50)], bytes([1, 240, 8, 1, 0, 0, 0, 0, 0, 0])))
    # Items cut short.
    v.append(vector("a value cut short", [item("voltage", 1, 3.92)], item("temperature", 1, 4.0)[0][:3]))
    v.append(vector("a header cut short", [item("voltage", 1, 3.92)], bytes([1])))
    v.append(vector("a position cut short", [], gps(1, 1.0, 2.0, 3.0)[0][:10]))
    # Longer than any answer (180 bytes): decoding goes on.
    v.append(vector("longer than an answer", [item("percentage", (i % 254) + 1, i % 101) for i in range(70)]))
    return v


def main():
    print("# CayenneLPP readings (protocol.md §16.7). Generated by lpp.py: do not edit.")
    for vec in vectors():
        print()
        print("[[lpp]]")
        print(f'name = "{vec["name"]}"')
        print(f'hex = "{vec["hex"]}"')
        print(f'consumed = {vec["consumed"]}')
        rows = []
        for it in vec["items"]:
            fields = ", ".join(f'{k} = "{x}"' if isinstance(x, str) else f"{k} = {x}" for k, x in it.items())
            rows.append("{ " + fields + " }")
        print("items = [" + (",\n  ".join(rows) if rows else "") + "]")


if __name__ == "__main__":
    main()
