# Digit recognizer: an optical neural network

The files behind Shine's optical neural network example
([docs/neural-network.md](../../docs/neural-network.md)):

| File | What it is |
|---|---|
| `train.py` | Trains the network on what the chip computes (JAX, scikit-learn) |
| `weights.json` | The trained weights W1 (16×16) and W2 (10×16), and the hidden scale H |
| `test-digits.json` | The 450 test digits as 4×4 thumbnails, with their labels |
| `digit-recognizer.shine` | The chip, as Shine builds it from the weights (`cargo run --example digits -- --write`) |
| `compare.py` | Runs the chip's layers in SAX and PhotonForge and compares them with Shine |

The digits are from the UCI Optical Recognition of Handwritten Digits dataset, as shipped
with scikit-learn: E. Alpaydin and C. Kaynak (1998), UCI Machine Learning Repository,
https://doi.org/10.24432/C50P49, under the Creative Commons Attribution 4.0 licence
(CC BY 4.0). `test-digits.json` holds 450 of them, averaged from 8×8 down to 4×4 pixels.
