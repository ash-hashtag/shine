//! What a design does to light between its ports, wavelength by wavelength:
//! power, phase and group delay of each input→output path, from its
//! transmission matrices ([`super::sim::transmission`]).

use crate::photonics::C64;

/// The speed of light, m/s.
const C: f64 = 299_792_458.0;

/// One input→output path over a range of wavelengths.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub wavelengths_nm: Vec<f64>,
    /// Transmitted power, as a fraction of the input (|t|²).
    pub power: Vec<f64>,
    /// Phase in radians, unwrapped along the wavelengths (no 2π jumps).
    pub phase: Vec<f64>,
    /// Group delay in ps: dφ/dω, the time a pulse takes along the path.
    /// (Shine's fields go as e^{+iβL}, so phase grows with frequency.)
    pub group_delay_ps: Vec<f64>,
}

/// Phase without 2π jumps between neighbouring samples.
pub fn unwrap(phases: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(phases.len());
    let mut offset = 0.0;
    for (k, &p) in phases.iter().enumerate() {
        if k > 0 {
            let step = p - phases[k - 1];
            offset -= (step / std::f64::consts::TAU).round() * std::f64::consts::TAU;
        }
        out.push(p + offset);
    }
    out
}

/// The path from samples of its field `t` at `wavelengths_nm` (ascending).
pub fn path(wavelengths_nm: &[f64], t: &[C64]) -> Path {
    let n = wavelengths_nm.len().min(t.len());
    let power: Vec<f64> = t[..n].iter().map(|v| v.norm_sqr()).collect();
    let phase = unwrap(&t[..n].iter().map(|v| v.arg()).collect::<Vec<_>>());
    // ω = 2πc/λ; central differences inside, one-sided at the ends.
    let omega: Vec<f64> = wavelengths_nm[..n].iter().map(|&wl| std::f64::consts::TAU * C / (wl * 1e-9)).collect();
    let delay = |a: usize, b: usize| {
        let dw = omega[b] - omega[a];
        if dw == 0.0 { 0.0 } else { (phase[b] - phase[a]) / dw * 1e12 }
    };
    let group_delay_ps = (0..n)
        .map(|k| match (k, n) {
            (_, 0 | 1) => 0.0,
            (0, _) => delay(0, 1),
            (k, n) if k == n - 1 => delay(n - 2, n - 1),
            (k, _) => delay(k - 1, k + 1),
        })
        .collect();
    Path { wavelengths_nm: wavelengths_nm[..n].to_vec(), power, phase, group_delay_ps }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photonics::{Circuit, blocks, components::Waveguide};

    #[test]
    fn unwrapping_removes_the_jumps() {
        let wrapped = [3.0, -3.0, -2.5, 3.1, 2.0];
        let u = unwrap(&wrapped);
        for w in u.windows(2) {
            assert!((w[1] - w[0]).abs() < std::f64::consts::PI, "{u:?}");
        }
        assert_eq!(u[0], 3.0);
    }

    #[test]
    fn a_waveguides_group_delay_is_its_length_over_the_group_velocity() {
        // 1 mm of silicon strip, n_g = 4.2: 1e-3 m × 4.2 / c = 14.0 ps.
        let mut c = Circuit::new();
        let wg = c.add(Waveguide::new(1000.0, 2.4).with_group_index(4.2, 1550.0));
        let wls: Vec<f64> = (0..201).map(|k| 1540.0 + 0.1 * k as f64).collect();
        let t: Vec<C64> = wls.iter().map(|&wl| c.transfer_matrix(wl, &[wg.input(0)], &[wg.output(0)]).unwrap()[(0, 0)]).collect();
        let p = path(&wls, &t);
        let expected = 1e-3 * 4.2 / C * 1e12;
        for d in &p.group_delay_ps {
            assert!((d - expected).abs() < 1e-6, "{d} ps vs {expected}");
        }
        assert!(p.power.iter().all(|&x| (x - 1.0).abs() < 1e-12), "lossless");
    }

    #[test]
    fn a_ring_delays_light_most_at_resonance() {
        // An over-coupled all-pass ring passes almost everything, but holds
        // light longest on resonance: the delay peaks where the phase turns fastest.
        let mut c = Circuit::new();
        let ring = c
            .add_block(&blocks::AllPassRing { coupling: 0.3, length_um: 100.0, n_eff: 2.4, loss_db_per_cm: 0.0, n_g: 4.2, reference_nm: 1550.0 })
            .unwrap();
        let wls: Vec<f64> = (0..4001).map(|k| 1545.0 + 10.0 * k as f64 / 4000.0).collect();
        let t: Vec<C64> = wls.iter().map(|&wl| c.transfer_matrix(wl, &[ring.inputs[0]], &[ring.outputs[0]]).unwrap()[(0, 0)]).collect();
        let p = path(&wls, &t);
        let (k_max, d_max) = p.group_delay_ps.iter().enumerate().fold((0, f64::MIN), |a, (k, &d)| if d > a.1 { (k, d) } else { a });
        // Lossless: |t| = 1 everywhere; the extra delay on resonance is the
        // round trip (L n_g / c = 1.4 ps) times (1 + r)/(1 − r).
        assert!(p.power.iter().all(|&x| (x - 1.0).abs() < 1e-9));
        let r = (1.0f64 - 0.3).sqrt();
        let round_trip = 100e-6 * 4.2 / C * 1e12;
        let expected = round_trip * (1.0 + r) / (1.0 - r);
        assert!((d_max - expected).abs() / expected < 0.02, "peak {d_max} ps at {} nm vs {expected}", p.wavelengths_nm[k_max]);
    }
}
