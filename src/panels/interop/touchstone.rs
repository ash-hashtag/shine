//! Touchstone (`.sNp`) S-parameter files, version 1: the format circuit
//! simulators, foundry kits and measurement software exchange.
//!
//! Shine's light is one-way, from a component's inputs to its outputs, so
//! its S-matrix has the inputs as ports `1..=m` and the outputs as ports
//! `m+1..=m+n`, with transmission `S[m+o][i]` and zeros elsewhere (nothing is
//! reflected or travels backwards). Frequencies are in Hz (`c / λ`),
//! ascending, and the values are real and imaginary parts of the field.

use crate::photonics::{C64, Matrix};

/// The speed of light, m/s.
pub const C: f64 = 299_792_458.0;

pub fn frequency_hz(wavelength_nm: f64) -> f64 {
    C / (wavelength_nm * 1e-9)
}

pub fn wavelength_nm(frequency_hz: f64) -> f64 {
    C / frequency_hz * 1e9
}

/// One frequency's full S-matrix (ports × ports).
#[derive(Clone, Debug, PartialEq)]
pub struct SParameters {
    pub ports: usize,
    /// Ascending, in Hz.
    pub frequencies_hz: Vec<f64>,
    pub matrices: Vec<Matrix>,
}

impl SParameters {
    /// The S-matrix of a one-way component: `transmission[k][(o, i)]` from
    /// input `i` to output `o` at `wavelengths_nm[k]`.
    pub fn from_transmission(wavelengths_nm: &[f64], transmission: &[Matrix]) -> Self {
        let (outputs, inputs) = transmission.first().map_or((0, 0), |t| (t.nrows(), t.ncols()));
        let ports = inputs + outputs;
        let mut points: Vec<(f64, Matrix)> = wavelengths_nm
            .iter()
            .zip(transmission)
            .map(|(&wl, t)| {
                let s = Matrix::from_fn(ports, ports, |r, c| if r >= inputs && c < inputs { t[(r - inputs, c)] } else { C64::ZERO });
                (frequency_hz(wl), s)
            })
            .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        points.dedup_by(|a, b| a.0 == b.0);
        let (frequencies_hz, matrices) = points.into_iter().unzip();
        Self { ports, frequencies_hz, matrices }
    }

    /// The file's text. `comments` go at the top, one `!` line each.
    pub fn to_touchstone(&self, comments: &[String]) -> String {
        let mut out = String::new();
        for c in comments {
            for line in c.lines() {
                out += &format!("! {line}\n");
            }
        }
        out += "# HZ S RI R 50\n";
        let n = self.ports;
        let pair = |v: C64| format!("{:.9e} {:.9e}", v.re, v.im);
        for (f, s) in self.frequencies_hz.iter().zip(&self.matrices) {
            let freq = format!("{f:.12e}");
            match n {
                // Two ports have their own order: S11 S21 S12 S22.
                2 => out += &format!("{freq} {} {} {} {}\n", pair(s[(0, 0)]), pair(s[(1, 0)]), pair(s[(0, 1)]), pair(s[(1, 1)])),
                _ => {
                    // Row by row, at most four values a line.
                    for r in 0..n {
                        for (k, chunk) in (0..n).collect::<Vec<_>>().chunks(4).enumerate() {
                            let lead = if r == 0 && k == 0 { freq.clone() } else { " ".repeat(freq.len()) };
                            let values: Vec<String> = chunk.iter().map(|&c| pair(s[(r, c)])).collect();
                            out += &format!("{lead} {}\n", values.join(" "));
                        }
                    }
                }
            }
        }
        out
    }

    /// Read a version 1 Touchstone file with `ports` ports (from its `.sNp`
    /// extension). Accepts any frequency unit and RI, MA or DB values.
    pub fn parse(text: &str, ports: usize) -> Result<Self, String> {
        if ports == 0 {
            return Err("no ports".into());
        }
        let mut unit = 1e9; // Touchstone's default is GHz.
        let mut format = "MA";
        let mut numbers: Vec<f64> = Vec::new();
        for line in text.lines() {
            let line = line.split('!').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(options) = line.strip_prefix('#') {
                for word in options.split_whitespace() {
                    match word.to_uppercase().as_str() {
                        "HZ" => unit = 1.0,
                        "KHZ" => unit = 1e3,
                        "MHZ" => unit = 1e6,
                        "GHZ" => unit = 1e9,
                        "THZ" => unit = 1e12,
                        "RI" => format = "RI",
                        "MA" => format = "MA",
                        "DB" => format = "DB",
                        "S" | "R" => {}
                        "Y" | "Z" | "H" | "G" => return Err("only S-parameters can be read".into()),
                        _ => {}
                    }
                }
                continue;
            }
            if line.starts_with('[') {
                return Err("Touchstone 2 files aren't supported yet; save it as version 1".into());
            }
            for word in line.split_whitespace() {
                numbers.push(word.parse().map_err(|_| format!("not a number: {word}"))?);
            }
        }
        let per_point = 1 + 2 * ports * ports;
        if numbers.is_empty() || !numbers.len().is_multiple_of(per_point) {
            return Err(format!("expected {per_point} numbers per frequency for {ports} ports, found {} in all", numbers.len()));
        }
        let value = |a: f64, b: f64| match format {
            "RI" => C64::new(a, b),
            "DB" => C64::from_polar(10f64.powf(a / 20.0), b.to_radians()),
            _ => C64::from_polar(a, b.to_radians()),
        };
        let mut frequencies_hz = Vec::new();
        let mut matrices = Vec::new();
        for point in numbers.chunks(per_point) {
            frequencies_hz.push(point[0] * unit);
            let at = |k: usize| value(point[1 + 2 * k], point[2 + 2 * k]);
            matrices.push(if ports == 2 {
                let (s11, s21, s12, s22) = (at(0), at(1), at(2), at(3));
                Matrix::from_fn(2, 2, |r, c| [[s11, s12], [s21, s22]][r][c])
            } else {
                Matrix::from_fn(ports, ports, |r, c| at(r * ports + c))
            });
        }
        if frequencies_hz.windows(2).any(|w| w[1] <= w[0]) {
            return Err("frequencies must increase".into());
        }
        Ok(Self { ports, frequencies_hz, matrices })
    }
}

/// The port count of a `.sNp` file name.
pub fn ports_of(name: &str) -> Option<usize> {
    let ext = name.rsplit_once('.')?.1.to_lowercase();
    ext.strip_prefix('s')?.strip_suffix('p')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transmission(inputs: usize, outputs: usize, seed: f64) -> Matrix {
        Matrix::from_fn(outputs, inputs, |o, i| C64::new(0.1 * (o + 1) as f64 + seed, -0.05 * (i + 1) as f64))
    }

    #[test]
    fn writes_and_reads_back_every_port_count() {
        for (inputs, outputs) in [(1, 1), (1, 2), (2, 2), (4, 4), (3, 5)] {
            let wls = [1550.0, 1500.0, 1600.0];
            let t: Vec<Matrix> = wls.iter().map(|&wl| transmission(inputs, outputs, wl / 1e4)).collect();
            let s = SParameters::from_transmission(&wls, &t);
            assert_eq!(s.ports, inputs + outputs);
            assert!(s.frequencies_hz.windows(2).all(|w| w[0] < w[1]), "ascending frequency");
            assert!((wavelength_nm(s.frequencies_hz[0]) - 1600.0).abs() < 1e-9, "longest wavelength first");
            let text = s.to_touchstone(&["test".into()]);
            let back = SParameters::parse(&text, s.ports).unwrap();
            assert_eq!(back.frequencies_hz.len(), 3);
            for (a, b) in s.matrices.iter().zip(&back.matrices) {
                for r in 0..s.ports {
                    for c in 0..s.ports {
                        assert!((a[(r, c)] - b[(r, c)]).norm() < 1e-8, "{inputs}x{outputs} S{}{}", r + 1, c + 1);
                    }
                }
            }
            // Inputs to outputs only.
            let m = &back.matrices[0];
            assert_eq!(m[(0, 0)], C64::ZERO);
            assert_ne!(m[(inputs, 0)], C64::ZERO);
        }
    }

    #[test]
    fn two_ports_use_the_s11_s21_s12_s22_order() {
        let t = [Matrix::from_fn(1, 1, |_, _| C64::new(0.5, 0.25))];
        let text = SParameters::from_transmission(&[1550.0], &t).to_touchstone(&[]);
        let data: Vec<&str> = text.lines().last().unwrap().split_whitespace().collect();
        assert_eq!(data.len(), 9);
        assert_eq!((data[3].parse::<f64>().unwrap(), data[4].parse::<f64>().unwrap()), (0.5, 0.25), "S21 second");
        assert_eq!(data[5].parse::<f64>().unwrap(), 0.0, "S12 third");
    }

    #[test]
    fn reads_other_units_and_formats() {
        let text = "! a measured 2-port\n# GHz S DB R 50\n193000 -3.0103 0 -3.0103 90 -100 0 -100 0\n";
        let s = SParameters::parse(text, 2).unwrap();
        assert_eq!(s.frequencies_hz, vec![1.93e14]);
        assert!((s.matrices[0][(0, 0)].norm_sqr() - 0.5).abs() < 1e-4);
        assert!((s.matrices[0][(1, 0)].arg() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(SParameters::parse("# GHz Z RI R 50\n1 0 0", 1).is_err());
        assert!(SParameters::parse("1 2 3", 2).is_err());
        assert_eq!(ports_of("ring.s4p"), Some(4));
        assert_eq!(ports_of("ring.S2P"), Some(2));
        assert_eq!(ports_of("ring.txt"), None);
    }
}
