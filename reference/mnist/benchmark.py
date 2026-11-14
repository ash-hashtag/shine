"""Benchmark Shine's Fashion-MNIST chip on the CPU, with the `shine` program.

Runs Fashion-MNIST's test pictures through the whole chip (light, detectors and
electronics) in one `shine simulate --input -` run, and reports how long it took,
per picture, the peak memory, the chip's accuracy, and how closely it matches the
trained network (its weights, fashion-mnist-weights.json, computed here with NumPy).

    pip install numpy
    python reference/mnist/benchmark.py [--images 1000] [--shine PROGRAM] [--cache DIR]

The test pictures (Fashion-MNIST, Zalando Research, MIT licence) are downloaded
into the cache folder (default ~/.cache/shine/fashion-mnist) the first time.
"""
import argparse, csv, gzip, io, json, os, platform, resource, shutil, subprocess, sys, time, urllib.request

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
MIRROR = "http://fashion-mnist.s3-website.eu-central-1.amazonaws.com/"


def test_set(cache):
    """Fashion-MNIST's 10 000 test pictures (values 0 to 1) and their labels."""
    os.makedirs(cache, exist_ok=True)
    data = []
    for name in ["t10k-images-idx3-ubyte", "t10k-labels-idx1-ubyte"]:
        path = os.path.join(cache, name + ".gz")
        if not os.path.exists(path):
            urllib.request.urlretrieve(MIRROR + name + ".gz", path)
        data.append(gzip.open(path).read())
    images = np.frombuffer(data[0], np.uint8, offset=16).reshape(-1, 784).astype(np.float64) / 255.0
    labels = np.frombuffer(data[1], np.uint8, offset=8).astype(np.int64)
    return images, labels


def network(w, x):
    """The trained network's scores, as the chip computes them: |W2 clamp(|W1 x|²/H)|²."""
    h = np.clip((x @ np.asarray(w["w1"]).T) ** 2 / w["hidden_scale"], 0, 1)
    return (h @ np.asarray(w["w2"]).T) ** 2


def run_chip(shine, chip, images):
    """The chip's scores and classes for `images`, in one run of `shine`, and the seconds it took."""
    stream = "".join(" ".join(repr(float(v)) for v in row) + "\n" for row in images)
    command = [shine, "simulate", chip, "--input", "-", "--csv", "--box-outputs", "--only", "Layer 2", "--only", "Display"]
    started = time.perf_counter()
    done = subprocess.run(command, input=stream, capture_output=True, text=True)
    seconds = time.perf_counter() - started
    if done.returncode != 0:
        sys.exit(done.stderr.strip())
    rows = list(csv.reader(io.StringIO(done.stdout)))
    if len(images) == 1:
        readings = {name: float(v) for name, v, _ in rows[1:]}
        table = [readings]
    else:
        names = rows[0][1:]
        table = [dict(zip(names, map(float, r[1:]))) for r in rows[1:]]
    scores = np.array([[v for k, v in r.items() if k.startswith("Layer 2")] for r in table])
    classes = np.array([int(next(v for k, v in r.items() if k.startswith("Display"))) for r in table])
    return scores, classes, seconds


def main():
    a = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    a.add_argument("--images", type=int, default=1000, help="how many test pictures (up to 10 000)")
    a.add_argument("--shine", default=os.environ.get("SHINE") or shutil.which("shine"), help="the shine program")
    a.add_argument("--chip", default=os.path.join(HERE, "fashion-mnist-chip.shine"))
    a.add_argument("--weights", default=os.path.join(HERE, "fashion-mnist-weights.json"))
    a.add_argument("--cache", default=os.path.join(os.path.expanduser("~"), ".cache", "shine", "fashion-mnist"))
    args = a.parse_args()
    if not args.shine:
        sys.exit("Can't find the shine program: put it on PATH, set SHINE, or give --shine")

    images, labels = test_set(args.cache)
    n = min(args.images, len(labels))
    x, y = images[:n], labels[:n]
    w = json.load(open(args.weights))
    started = time.perf_counter()
    want = network(w, x)
    digital = time.perf_counter() - started

    # One picture on its own: the first costs solving the meshes into their matrices.
    _, _, first = run_chip(args.shine, args.chip, x[:1])
    scores, classes, total = run_chip(args.shine, args.chip, x)
    peak_mb = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss / 1024

    # The chip's scores are the network's times one factor (laser power, splitting, losses).
    factor = (scores * want).sum() / (want * want).sum()
    worst = float(np.max(np.abs(scores - factor * want)) / np.max(np.abs(factor * want)))
    rest = (total - first) / max(n - 1, 1)

    print(f"Fashion-MNIST chip ({os.path.basename(args.chip)}), {n} test pictures, on {platform.processor() or platform.machine()} ({os.cpu_count()} threads)")
    print()
    print("| | |")
    print("|---|---|")
    print(f"| One picture (opening the chip, solving its meshes, simulating) | {first:.2f} s |")
    print(f"| {n} pictures in one run | {total:.1f} s |")
    print(f"| … each picture after the first | {1000 * rest:.0f} ms ({1 / rest:.0f} pictures a second) |")
    print(f"| Peak memory | {peak_mb:.0f} MB |")
    print(f"| The same network computed directly (NumPy, for scale) | {1e6 * digital / n:.0f} µs a picture |")
    print(f"| Accuracy | {100 * np.mean(classes == y):.2f}% (the trained network: {100 * np.mean(want.argmax(1) == y):.2f}%) |")
    print(f"| The network's class | {int(np.sum(classes == want.argmax(1)))} of {n} |")
    print(f"| Scores, largest difference from the network's | {worst:.1e} of the largest |")


if __name__ == "__main__":
    main()
