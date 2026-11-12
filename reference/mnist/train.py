"""Train an optical neural network on 28×28 pictures: Fashion-MNIST (the
default) or MNIST.

Shine's chip for it (`cargo run --release --example mnist`) computes, with
light, detectors and electronics:

    x     = the 28×28 image, 784 values in [0, 1] (light amplitudes)
    h     = clamp(|W1 x|² / H, 0, 1)      optical layer 1, photodetectors, neurons
    o     = |W2 h|²                        optical layer 2, photodetectors
    digit = argmax(o)

This trains W1 (HIDDEN × 784) and W2 (10 × HIDDEN) on exactly that, so the
weights suit the hardware.

The data isn't kept in the repository: this downloads it into the cache
folder given, and writes the weights and the test set there too.
Fashion-MNIST (Han Xiao, Kashif Rasul and Roland Vollgraf, Zalando Research,
2017) is under the MIT licence, so the chip trained on it ships with Shine;
MNIST (LeCun, Cortes and Burges) is under CC BY-SA 3.0.

    pip install numpy jax
    python reference/mnist/train.py CACHE_DIR [--dataset fashion|mnist] [--hidden 64] [--epochs 12]

Writes CACHE_DIR/weights.json, CACHE_DIR/test-images.f32 (10000 × 784,
little-endian float32, row by row) and CACHE_DIR/test-labels.u8.
"""
import argparse, gzip, json, os, time, urllib.request

import numpy as np
import jax, jax.numpy as jnp

DATASETS = {
    "fashion": ("Fashion-MNIST", "http://fashion-mnist.s3-website.eu-central-1.amazonaws.com/",
                ["T-shirt/top", "Trouser", "Pullover", "Dress", "Coat", "Sandal", "Shirt", "Sneaker", "Bag", "Ankle boot"]),
    "mnist": ("MNIST", "https://ossci-datasets.s3.amazonaws.com/mnist/", [str(d) for d in range(10)]),
}
FILES = ["train-images-idx3-ubyte", "train-labels-idx1-ubyte", "t10k-images-idx3-ubyte", "t10k-labels-idx1-ubyte"]


def load(cache, mirror, name):
    path = os.path.join(cache, name + ".gz")
    if not os.path.exists(path):
        urllib.request.urlretrieve(mirror + name + ".gz", path)
    data = gzip.open(path).read()
    if "images" in name:
        n, rows, cols = np.frombuffer(data, ">i4", 3, 4)
        return np.frombuffer(data, np.uint8, offset=16).reshape(n, rows * cols).astype(np.float32) / 255.0
    return np.frombuffer(data, np.uint8, offset=8).astype(np.int64)


def forward(p, x, scale):
    h = jnp.clip((x @ p["w1"].T) ** 2 / scale, 0, 1)
    return (h @ p["w2"].T) ** 2


def loss(p, x, y, scale):
    o = forward(p, x, scale)
    logits = p["t"] * o / (o.sum(axis=1, keepdims=True) + 1e-9)
    return -jnp.mean(jax.nn.log_softmax(logits)[jnp.arange(len(y)), y])


def main():
    a = argparse.ArgumentParser()
    a.add_argument("cache")
    a.add_argument("--dataset", choices=sorted(DATASETS), default="fashion")
    a.add_argument("--hidden", type=int, default=64)
    a.add_argument("--epochs", type=int, default=12)
    a.add_argument("--batch", type=int, default=256)
    args = a.parse_args()
    title, mirror, classes = DATASETS[args.dataset]
    # Each dataset in a folder of its own: their files have the same names.
    data = os.path.join(args.cache, args.dataset)
    os.makedirs(data, exist_ok=True)
    xtr, ytr, xte, yte = (load(data, mirror, f) for f in FILES)

    key = jax.random.PRNGKey(0)
    k1, k2 = jax.random.split(key)
    p = {
        "w1": jax.random.normal(k1, (args.hidden, 784)) / np.sqrt(784),
        "w2": jax.random.normal(k2, (10, args.hidden)) / np.sqrt(args.hidden),
        "t": jnp.array(10.0),
    }
    grad = jax.jit(jax.value_and_grad(loss))
    m = jax.tree.map(jnp.zeros_like, p)
    v = jax.tree.map(jnp.zeros_like, p)
    lr, b1, b2, step = 2e-3, 0.9, 0.999, 0
    rng = np.random.default_rng(0)
    scale_of = lambda p: float(jnp.max((jnp.asarray(xtr[:5000]) @ p["w1"].T) ** 2))
    started = time.time()
    for epoch in range(args.epochs):
        scale = scale_of(p)   # the electronics' scale, refreshed each epoch
        order = rng.permutation(len(ytr))
        for k in range(0, len(order), args.batch):
            idx = order[k : k + args.batch]
            step += 1
            _, g = grad(p, jnp.asarray(xtr[idx]), jnp.asarray(ytr[idx]), scale)
            m = jax.tree.map(lambda m, g: b1 * m + (1 - b1) * g, m, g)
            v = jax.tree.map(lambda v, g: b2 * v + (1 - b2) * g * g, v, g)
            p = jax.tree.map(lambda p, m, v: p - lr * (m / (1 - b1**step)) / (jnp.sqrt(v / (1 - b2**step)) + 1e-8), p, m, v)
        print(f"epoch {epoch + 1}: {time.time() - started:.0f} s")

    # The hidden scale over the whole training set: the neurons' full range.
    scale = float(max(jnp.max((jnp.asarray(xtr[k : k + 10000]) @ p["w1"].T) ** 2) for k in range(0, len(xtr), 10000)))
    accuracy = lambda x, y: float(np.mean(np.argmax(np.asarray(forward(p, jnp.asarray(x), scale)), axis=1) == y))
    train_acc, test_acc = accuracy(xtr, ytr), accuracy(xte, yte)
    print(f"optical network 784 → {args.hidden} → 10 on {title}: train {train_acc:.4f}, test {test_acc:.4f}")
    json.dump(
        {
            "w1": np.asarray(p["w1"], dtype=float).tolist(),
            "w2": np.asarray(p["w2"], dtype=float).tolist(),
            "hidden_scale": scale,
            "train_accuracy": train_acc,
            "test_accuracy": test_acc,
            "dataset": title,
            "classes": classes,
        },
        open(os.path.join(args.cache, "weights.json"), "w"),
    )
    xte.astype("<f4").tofile(os.path.join(args.cache, "test-images.f32"))
    yte.astype(np.uint8).tofile(os.path.join(args.cache, "test-labels.u8"))


if __name__ == "__main__":
    main()
