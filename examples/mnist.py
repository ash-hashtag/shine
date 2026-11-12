#!/usr/bin/env python3
"""Read handwritten digits with Shine's MNIST chip.

Turns pictures of digits into the 784 pixel values the chip takes, prepared as
MNIST's digits were, and runs the `shine` program on the chip with them:

    shine simulate mnist-neural-chip.shine --input -     (the pixels on standard input)

The chip is a Shine project, reference/mnist/mnist-neural-chip.shine: an
optical neural network of 3.1 million parts (see docs/neural-network.md).
With Pillow, to read pictures (pip install pillow):

    python examples/mnist.py --image digit.png [more.png ...] [--chip FILE] [--shine PROGRAM]

shows the 28×28 the chip sees and the digit it reads, with the light reaching
each digit's detector. Several pictures go through one run of `shine`, one after
another. `--print-inputs` only prints the pixel values, a picture per line, for
`shine` yourself:

    python examples/mnist.py --image digit.png --print-inputs | shine simulate reference/mnist/mnist-neural-chip.shine --input -
"""
import argparse, csv, io, os, shutil, subprocess, sys

CHIP = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "reference", "mnist", "mnist-neural-chip.shine")

SIDE, BOX = 28, 20  # MNIST: 28×28 pictures, the digit fitted in 20×20


def flat(image):
    """An image's pixels, row by row (Pillow 12 renamed getdata)."""
    return list(image.get_flattened_data() if hasattr(image, "get_flattened_data") else image.getdata())


def pixels(path):
    """A picture of a digit as MNIST's are: 784 values from 0 (paper) to 1 (ink),
    row by row, the digit fitted in 20×20 with its centre of mass in the middle."""
    try:
        from PIL import Image
    except ImportError:
        sys.exit("Reading pictures needs Pillow: pip install pillow")
    picture = Image.open(path).convert("LA")  # brightness and transparency
    w, h = picture.size
    data = flat(picture)
    # Transparent parts count as white paper.
    bright = [(l * a + 255 * (255 - a)) / 255 / 255 for l, a in data]
    # The paper is what's around the edges; the ink is what differs from it.
    edge = [bright[y * w + x] for x in range(w) for y in (0, h - 1)] + [bright[y * w + x] for y in range(h) for x in (0, w - 1)]
    light_paper = sum(edge) / len(edge) > 0.5
    ink = [1 - b if light_paper else b for b in bright]
    peak = max(ink)
    if peak <= 0.05:
        sys.exit(f"{path}: no digit in it")
    # The digit's bounds: where the ink is at least a fifth of its darkest.
    inked = [i >= 0.2 * peak for i in ink]
    xs = [x for x in range(w) if any(inked[y * w + x] for y in range(h))]
    ys = [y for y in range(h) if any(inked[y * w + x] for x in range(w))]
    x0, x1, y0, y1 = xs[0], xs[-1], ys[0], ys[-1]
    crop = Image.new("L", (x1 - x0 + 1, y1 - y0 + 1))
    crop.putdata([round(255 * ink[y * w + x] / peak) for y in range(y0, y1 + 1) for x in range(x0, x1 + 1)])
    # The longest side 20 pixels.
    scale = BOX / max(crop.size)
    small = crop.resize((max(1, round(crop.width * scale)), max(1, round(crop.height * scale))), Image.LANCZOS)
    sw, sh = small.size
    values = flat(small)
    mass = sum(values)
    cx = sum(v * (k % sw) for k, v in enumerate(values)) / mass
    cy = sum(v * (k // sw) for k, v in enumerate(values)) / mass
    dx, dy = round(SIDE / 2 - cx), round(SIDE / 2 - cy)
    out = [0.0] * (SIDE * SIDE)
    for k, v in enumerate(values):
        x, y = k % sw + dx, k // sw + dy
        if 0 <= x < SIDE and 0 <= y < SIDE:
            out[y * SIDE + x] = v / 255
    return out


def preview(values):
    """The 28×28 as text, denser characters for more ink."""
    shades = " .:-=+*#%@"
    return "\n".join("".join(2 * shades[min(9, round(v * 9))] for v in values[r : r + SIDE]) for r in range(0, SIDE * SIDE, SIDE))


def find_shine(given):
    program = given or os.environ.get("SHINE") or shutil.which("shine")
    if not program:
        sys.exit("Can't find the shine program: put it on PATH, set SHINE, or give --shine "
                 "(built with `cargo build --release`, it is target/release/shine)")
    return program


def main():
    a = argparse.ArgumentParser(description="Read handwritten digits with Shine's MNIST chip.")
    a.add_argument("--image", nargs="+", required=True, help="pictures of digits (PNG, JPEG, ...)")
    a.add_argument("--chip", default=os.environ.get("SHINE_MNIST_CHIP", os.path.normpath(CHIP)),
                   help="the chip (default reference/mnist/mnist-neural-chip.shine, or SHINE_MNIST_CHIP)")
    a.add_argument("--shine", help="the shine program (default: SHINE, else shine on PATH)")
    a.add_argument("--print-inputs", action="store_true", help="only print the pixel values, a picture per line")
    args = a.parse_args()

    inputs = [pixels(p) for p in args.image]
    stream = "".join(" ".join(f"{v:.4f}" for v in values) + "\n" for values in inputs)
    if args.print_inputs:
        sys.stdout.write(stream)
        return
    if not os.path.exists(args.chip):
        sys.exit(f"No chip at {args.chip}: see reference/mnist/README.md to make it, or give --chip")

    # The ten scores are the light leaving layer 2; the display shows the digit.
    command = [find_shine(args.shine), "simulate", args.chip, "--input", "-", "--csv", "--box-outputs",
               "--only", "Layer 2", "--only", "Display"]
    run = subprocess.run(command, input=stream, capture_output=True, text=True)
    if run.returncode != 0:
        sys.exit(run.stderr.strip() or f"shine exited with {run.returncode}")
    rows = list(csv.reader(io.StringIO(run.stdout)))
    if len(inputs) == 1:
        # One picture: a reading per row (name, value, unit).
        results = [{name: float(value) for name, value, _ in rows[1:]}]
    else:
        # Several: a row per picture, a column per reading.
        names = rows[0][1:]
        results = [dict(zip(names, map(float, row[1:]))) for row in rows[1:]]

    for path, values, readings in zip(args.image, inputs, results):
        scores = [v for name, v in readings.items() if name.startswith("Layer 2")]
        digit = int(next(v for name, v in readings.items() if name.startswith("Display")))
        total = sum(scores) or 1.0
        print(f"{path}: what the chip sees")
        print(preview(values))
        print("Light at each digit's detector:")
        for d, s in enumerate(scores):
            print(f"  {d}: {s:9.3e} mW {100 * s / total:5.1f}%  {'█' * round(40 * s / total)}{'  ←' if d == digit else ''}")
        print(f"The chip reads {digit}\n")


if __name__ == "__main__":
    main()
