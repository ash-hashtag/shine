"""Train the optical neural network of Shine's digits example
(`cargo run --example digits`): a photonic neural network for the UCI handwritten digits (CC BY 4.0,
Alpaydin & Kaynak 1998, as shipped with scikit-learn), on exactly the maths
the chip does:

    x  = the digit as a 4×4 thumbnail, 16 values in [0, 1] (light amplitudes)
    h  = clip(|W1 x|² / H, 0, 1)     optical layer 1, photodetectors, electronics
    o  = |W2 h|²                      optical layer 2 (on fresh light), detectors
    digit = argmax(o)

W1 (16×16) and W2 (10×16) are real; the chip realises each as two MZI meshes
with attenuators between them (singular value decomposition).

    pip install numpy jax scikit-learn
    python reference/digits/train.py reference/digits
"""
import json, sys, time
import numpy as np, jax, jax.numpy as jnp
from sklearn.datasets import load_digits

jax.config.update("jax_enable_x64", True)
d = load_digits()
img = d.images / 16.0                                     # 8×8, 0..1
x = img.reshape(-1, 4, 2, 4, 2).mean(axis=(2, 4)).reshape(-1, 16)  # 4×4 thumbnails
y = d.target
rng = np.random.default_rng(0)
order = rng.permutation(len(y))
test, train = order[:450], order[450:]

def forward(p, x, H):
    h = jnp.clip((x @ p["w1"].T) ** 2 / H, 0, 1)
    return (h @ p["w2"].T) ** 2

def loss(p, x, y, H):
    o = forward(p, x, H)
    logits = p["t"] * o / (o.sum(axis=1, keepdims=True) + 1e-9)
    return -jnp.mean(jax.nn.log_softmax(logits)[jnp.arange(len(y)), y])

key = jax.random.PRNGKey(1)
k1, k2 = jax.random.split(key)
p = {"w1": jax.random.normal(k1, (16, 16)) * 0.5, "w2": jax.random.normal(k2, (10, 16)) * 0.5, "t": jnp.array(10.0)}
H = 1.0
grad = jax.jit(jax.value_and_grad(loss))
m = jax.tree.map(jnp.zeros_like, p); v = jax.tree.map(jnp.zeros_like, p)
lr, b1, b2 = 0.02, 0.9, 0.999
X, Y = jnp.array(x[train]), jnp.array(y[train])
for step in range(1, 6001):
    if step % 200 == 1:
        # The electronics' scale: the largest hidden power on the training set.
        H = float(jnp.max((X @ p["w1"].T) ** 2))
    l, g = grad(p, X, Y, H)
    m = jax.tree.map(lambda m, g: b1 * m + (1 - b1) * g, m, g)
    v = jax.tree.map(lambda v, g: b2 * v + (1 - b2) * g * g, v, g)
    p = jax.tree.map(lambda p, m, v: p - lr * (m / (1 - b1**step)) / (jnp.sqrt(v / (1 - b2**step)) + 1e-8), p, m, v)
H = float(jnp.max((X @ p["w1"].T) ** 2))
acc = lambda idx: float(np.mean(np.argmax(np.array(forward(p, jnp.array(x[idx]), H)), axis=1) == y[idx]))
print(f"photonic network: train {acc(train):.3f}  test {acc(test):.3f}  loss {float(l):.3f}")

# A digital network of the same size for reference: 16 → 16 ReLU → 10.
from sklearn.neural_network import MLPClassifier
mlp = MLPClassifier(hidden_layer_sizes=(16,), max_iter=3000, random_state=0).fit(x[train], y[train])
print(f"digital MLP (ReLU, biases): test {mlp.score(x[test], y[test]):.3f}")
# The weights, and the test digits (thumbnails and labels) for checking the chip.
out = sys.argv[1]
r = lambda a: np.round(np.asarray(a, dtype=float), 12).tolist()
json.dump({"w1": r(p["w1"]), "w2": r(p["w2"]), "hidden_scale": H, "train_accuracy": acc(train), "test_accuracy": acc(test)},
          open(f"{out}/weights.json", "w"), indent=1)
json.dump({"pixels": np.round(x[test], 6).tolist(), "labels": y[test].tolist()}, open(f"{out}/test-digits.json", "w"))
