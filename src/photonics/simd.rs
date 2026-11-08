//! Vectorised kernels over a batch of fields stored as split real/imaginary
//! lanes. Callers dispatch once (see [`pulp::Arch::dispatch`]) and pass the
//! selected `S: Simd` down so everything inlines with the right target features.

use pulp::Simd;

use super::C64;

/// `acc += c * x` for complex `c` and split-complex vectors `acc`, `x`.
#[inline(always)]
pub fn caxpy<S: Simd>(
    simd: S,
    acc_re: &mut [f64],
    acc_im: &mut [f64],
    x_re: &[f64],
    x_im: &[f64],
    c: C64,
) {
    debug_assert!(acc_re.len() == x_re.len() && acc_im.len() == x_im.len());
    let (ar, ar_tail) = S::as_mut_simd_f64s(acc_re);
    let (ai, ai_tail) = S::as_mut_simd_f64s(acc_im);
    let (xr, xr_tail) = S::as_simd_f64s(x_re);
    let (xi, xi_tail) = S::as_simd_f64s(x_im);

    let cr = simd.splat_f64s(c.re);
    if c.im == 0.0 {
        // Real coefficient (couplers' through ports, attenuators): half the work.
        for (((ar, ai), xr), xi) in ar.iter_mut().zip(ai.iter_mut()).zip(xr).zip(xi) {
            *ar = simd.mul_add_e_f64s(cr, *xr, *ar);
            *ai = simd.mul_add_e_f64s(cr, *xi, *ai);
        }
    } else {
        let ci = simd.splat_f64s(c.im);
        let neg_ci = simd.splat_f64s(-c.im);
        for (((ar, ai), xr), xi) in ar.iter_mut().zip(ai.iter_mut()).zip(xr).zip(xi) {
            *ar = simd.mul_add_e_f64s(cr, *xr, simd.mul_add_e_f64s(neg_ci, *xi, *ar));
            *ai = simd.mul_add_e_f64s(cr, *xi, simd.mul_add_e_f64s(ci, *xr, *ai));
        }
    }

    for (((ar, ai), xr), xi) in ar_tail
        .iter_mut()
        .zip(ai_tail.iter_mut())
        .zip(xr_tail)
        .zip(xi_tail)
    {
        *ar += c.re * xr - c.im * xi;
        *ai += c.re * xi + c.im * xr;
    }
}

/// Sum of `|x|^2` added lane-wise into `acc`.
#[inline(always)]
pub fn accumulate_power<S: Simd>(simd: S, acc: &mut [f64], x_re: &[f64], x_im: &[f64]) {
    let (a, a_tail) = S::as_mut_simd_f64s(acc);
    let (xr, xr_tail) = S::as_simd_f64s(x_re);
    let (xi, xi_tail) = S::as_simd_f64s(x_im);
    for ((a, xr), xi) in a.iter_mut().zip(xr).zip(xi) {
        *a = simd.mul_add_e_f64s(*xr, *xr, simd.mul_add_e_f64s(*xi, *xi, *a));
    }
    for ((a, xr), xi) in a_tail.iter_mut().zip(xr_tail).zip(xi_tail) {
        *a += xr * xr + xi * xi;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulp::{Arch, WithSimd};

    struct Caxpy<'a> {
        acc: (&'a mut [f64], &'a mut [f64]),
        x: (&'a [f64], &'a [f64]),
        c: C64,
    }

    impl WithSimd for Caxpy<'_> {
        type Output = ();
        fn with_simd<S: Simd>(self, simd: S) {
            caxpy(simd, self.acc.0, self.acc.1, self.x.0, self.x.1, self.c);
        }
    }

    #[test]
    fn caxpy_matches_scalar() {
        // Odd length exercises both the vector body and the scalar tail.
        let n = 37;
        let xr: Vec<f64> = (0..n).map(|i| i as f64 * 0.5).collect();
        let xi: Vec<f64> = (0..n).map(|i| 1.0 - i as f64).collect();
        for c in [C64::new(0.3, -1.2), C64::new(2.0, 0.0)] {
            let mut ar = vec![1.0; n];
            let mut ai = vec![-2.0; n];
            Arch::new().dispatch(Caxpy {
                acc: (&mut ar, &mut ai),
                x: (&xr, &xi),
                c,
            });
            for i in 0..n {
                let expected = C64::new(1.0, -2.0) + c * C64::new(xr[i], xi[i]);
                assert!((C64::new(ar[i], ai[i]) - expected).norm() < 1e-12);
            }
        }
    }

    struct Power<'a> {
        acc: &'a mut [f64],
        x: (&'a [f64], &'a [f64]),
    }

    impl WithSimd for Power<'_> {
        type Output = ();
        fn with_simd<S: Simd>(self, simd: S) {
            accumulate_power(simd, self.acc, self.x.0, self.x.1);
        }
    }

    #[test]
    fn accumulate_power_matches_scalar() {
        for n in [0, 1, 3, 8, 37] {
            let xr: Vec<f64> = (0..n).map(|i| i as f64 - 3.0).collect();
            let xi: Vec<f64> = (0..n).map(|i| 0.5 * i as f64).collect();
            let mut acc = vec![1.0; n];
            Arch::new().dispatch(Power {
                acc: &mut acc,
                x: (&xr, &xi),
            });
            for i in 0..n {
                assert!((acc[i] - (1.0 + xr[i] * xr[i] + xi[i] * xi[i])).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn caxpy_handles_empty_and_zero() {
        let (mut ar, mut ai) = (Vec::<f64>::new(), Vec::<f64>::new());
        Arch::new().dispatch(Caxpy {
            acc: (&mut ar, &mut ai),
            x: (&[], &[]),
            c: C64::new(1.0, 1.0),
        });
        let mut ar = vec![2.0; 9];
        let mut ai = vec![3.0; 9];
        Arch::new().dispatch(Caxpy {
            acc: (&mut ar, &mut ai),
            x: (&[5.0; 9], &[7.0; 9]),
            c: C64::ZERO,
        });
        assert!(ar.iter().all(|&v| v == 2.0) && ai.iter().all(|&v| v == 3.0));
    }

    #[test]
    fn caxpy_pure_imaginary_rotates() {
        let mut ar = vec![0.0; 10];
        let mut ai = vec![0.0; 10];
        Arch::new().dispatch(Caxpy {
            acc: (&mut ar, &mut ai),
            x: (&[1.0; 10], &[0.0; 10]),
            c: C64::new(0.0, 1.0),
        });
        assert!(ar.iter().all(|&v| v == 0.0) && ai.iter().all(|&v| v == 1.0));
    }
}
