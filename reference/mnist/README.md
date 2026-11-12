# MNIST digit recognizer: an optical neural network of 3.1 million parts

The full-size version of Shine's optical neural network
([docs/neural-network.md](../../docs/neural-network.md#mnist-at-full-size)): 28×28 pixels
into a 784-mode MZI mesh, then 64 optoelectronic neurons and a second layer to ten digits.

| File | What it is |
|---|---|
| `mnist-neural-chip.shine` | The chip, ready to simulate: `shine simulate mnist-neural-chip.shine --input -` with 784 pixel values, or `python examples/mnist.py --image digit.png` |
| `train.py` | Trains the network on what the chip computes (JAX), downloading MNIST into a cache folder |

The chip reads 97.40% of MNIST's 10 000 test digits, as the trained network does.

To make it again (the training data isn't kept here):

```sh
pip install numpy jax
python reference/mnist/train.py CACHE
cargo run --release --example mnist -- CACHE --write-chip reference/mnist/mnist-neural-chip.shine
```

## Licence

The chip's weights were trained on the MNIST database of handwritten digits (Y. LeCun,
C. Cortes and C. J. C. Burges), which is under the Creative Commons Attribution-ShareAlike
3.0 licence (CC BY-SA 3.0). So `mnist-neural-chip.shine` is shared under CC BY-SA 3.0 too,
https://creativecommons.org/licenses/by-sa/3.0/, rather than Shine's GPL; `train.py` is
part of Shine, under the GPL. MNIST itself isn't in the repository.
