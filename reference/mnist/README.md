# A 28×28 optical neural network of 3.1 million parts

The full-size version of Shine's optical neural network
([docs/neural-network.md](../../docs/neural-network.md#at-full-size)): 28×28 pixels into a
784-mode MZI mesh, then 64 optoelectronic neurons and a second layer to ten classes.

| File | What it is |
|---|---|
| `fashion-mnist-chip.shine` | The chip trained on Fashion-MNIST, ready to simulate: `shine simulate fashion-mnist-chip.shine --input -` with 784 pixel values, or `python examples/mnist.py --image shoe.png` |
| `fashion-mnist-weights.json` | Its trained weights (W1 64×784, W2 10×64, the hidden scale H): `shine network fashion-mnist-weights.json --out chip.shine` builds the chip from them |
| `benchmark.py` | Runs the test pictures through the chip with the `shine` program: time per picture, memory, accuracy, and agreement with the network (NumPy) |
| `train.py` | Trains the network on what the chip computes (JAX), on Fashion-MNIST or MNIST, downloading the data into a cache folder |

The chip reads 87.96% of Fashion-MNIST's 10 000 test pictures, as the trained network
does, giving the network's class for every one
([benchmarks](../../docs/neural-network.md#benchmarks)). Its classes, in the order of its display: 0 T-shirt/top, 1 trouser, 2 pullover,
3 dress, 4 coat, 5 sandal, 6 shirt, 7 sneaker, 8 bag, 9 ankle boot.

To make it again (the data isn't kept here):

```sh
pip install numpy jax
python reference/mnist/train.py CACHE --dataset fashion
shine network CACHE/weights.json --out reference/mnist/fashion-mnist-chip.shine
```

`--dataset mnist` trains it on MNIST's handwritten digits instead (97.40%).

## Licence

Fashion-MNIST is by Han Xiao, Kashif Rasul and Roland Vollgraf (Zalando Research, 2017,
https://github.com/zalandoresearch/fashion-mnist), under the MIT licence; the chip's
weights were trained on it. MNIST (Y. LeCun, C. Cortes and C. J. C. Burges) is under
CC BY-SA 3.0, so neither its digits nor a chip trained on them are kept here.
