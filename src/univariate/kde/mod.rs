//! Kernel density estimation

pub mod kernel;

use std::{ptr, thread};

use cast::From;
use num_cpus;
use simd::traits::Vector;

use Float;
use univariate::Sample;

use self::kernel::Kernel;

/// Univariate kernel density estimator
pub struct Kde<'a, A, K> where A: 'a + Float, K: Kernel<A> {
    bandwidth: A,
    kernel: K,
    sample: &'a Sample<A>,
}

impl<'a, A, K> Kde<'a, A, K> where A: 'a + Float, K: Kernel<A> {
    /// Creates a new kernel density estimator from the `sample`, using a kernel `k` and estimating
    /// the bandwidth using the method `bw`
    pub fn new(sample: &'a Sample<A>, k: K, bw: Bandwidth<A>) -> Kde<'a, A, K> {
        Kde {
            bandwidth: bw.estimate(sample),
            kernel: k,
            sample: sample,
        }
    }

    /// Returns the bandwidth used by the estimator
    pub fn bandwidth(&self) -> A {
        self.bandwidth
    }

    /// Maps the KDE over `xs`
    ///
    /// - Multihreaded
    pub fn map(&self, xs: &[A]) -> Box<[A]> {
        let n = xs.len();
        let ncpus = num_cpus::get();

        // TODO need some sensible threshold to trigger the multi-threaded path
        if ncpus > 1 && n > ncpus {
            let granularity = n / ncpus + 1;

            unsafe {
                let mut ys = Vec::with_capacity(n);
                ys.set_len(n);

                {
                    ys.chunks_mut(granularity).enumerate().map(|(i, ys)| {
                        let offset = i * granularity;

                        thread::scoped(move || {
                            for (i, y) in ys.iter_mut().enumerate() {
                                ptr::write(y, (self)(*xs.get_unchecked(offset + i)))
                            }
                        })
                    }).collect::<Vec<_>>();
                }

                ys.into_boxed_slice()
            }
        } else {
            xs.iter().map(|&x| (self)(x)).collect::<Vec<_>>().into_boxed_slice()
        }
    }
}

impl<'a, A, K> Fn<(A,)> for Kde<'a, A, K> where A: 'a + Float, K: Kernel<A> {
    /// Estimates the probability density of `x`
    ///
    /// - Acceleration: SIMD
    extern "rust-call" fn call(&self, (x,): (A,)) -> A {
        let slice = self.sample.as_slice();
        let h = self.bandwidth;
        let n = A::from(slice.len());

        let x_ = A::Vector::from_elem(x);
        let h_ = A::Vector::from_elem(h);
        let (head, body, tail) = A::Vector::cast(slice);

        let sum = body.iter().fold(A::Vector::zeroed(), |acc, &x_i| {
            acc + ((x_ - x_i) / h_).map(self.kernel)
        }).sum();

        let sum = head.iter().chain(tail.iter()).fold(sum, |acc, &x_i| {
            acc + (self.kernel)((x - x_i) / h)
        });

        sum / h / n
    }
}

impl<'a, A, K> FnMut<(A,)> for Kde<'a, A, K> where A: 'a + Float, K: Kernel<A> {
    extern "rust-call" fn call_mut(&mut self, args: (A,)) -> A {
        self.call(args)
    }
}

impl<'a, A, K> FnOnce<(A,)> for Kde<'a, A, K> where A: 'a + Float, K: Kernel<A> {
    type Output = A;

    extern "rust-call" fn call_once(self, args: (A,)) -> A {
        self.call(args)
    }
}

/// Method to estimate the bandwidth
pub enum Bandwidth<A> where A: Float {
    /// Use this value as the bandwidth
    Manual(A),
    /// Use Silverman's rule of thumb to estimate the bandwidth from the sample
    Silverman,
}

impl<A> Bandwidth<A> where A: Float {
    fn estimate(self, sample: &Sample<A>) -> A {
        match self {
            Bandwidth::Silverman => {
                let factor = A::from(4. / 3.);
                let exponent = A::from(1. / 5.);
                let n = A::from(sample.as_slice().len());
                let sigma = sample.std_dev(None);

                sigma * (factor / n).powf(exponent)
            },
            Bandwidth::Manual(bw) => bw,
        }
    }
}

macro_rules! test {
    ($ty:ident) => {
        mod $ty {
            use quickcheck::TestResult;

            use univariate::Sample;
            use univariate::kde::kernel::Gaussian;
            use univariate::kde::{Bandwidth, Kde};

            // FIXME flaky test for `f32`
            // The [-inf inf] integral of the estimated PDF should be one
            #[quickcheck]
            #[ignore]
            fn integral(size: usize, start: usize) -> TestResult {
                const DX: $ty = 1e-3;

                if let Some(v) = ::test::vec::<$ty>(size, start) {
                    let slice = &v[start..];
                    let data = Sample::new(slice);
                    let kde = Kde::new(data, Gaussian, Bandwidth::Silverman);
                    let h = kde.bandwidth();
                    // NB Obviously a [-inf inf] integral is not feasible, but this range works
                    // quite well
                    let (a, b) = (data.min() - 5. * h, data.max() + 5. * h);

                    let mut acc = 0.;
                    let mut x = a;
                    let mut y = kde(a);

                    while x < b {
                        acc += DX * y / 2.;

                        x += DX;
                        y = kde(x);

                        acc += DX * y / 2.;
                    }

                    TestResult::from_bool(approx_eq!(acc, 1.))
                } else {
                    TestResult::discard()
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    test!(f32);
    test!(f64);
}

macro_rules! bench {
    ($ty:ident) => {
        mod $ty {

            use stdtest::Bencher;

            use univariate::Sample;
            use univariate::kde::kernel::Gaussian;
            use univariate::kde::{Bandwidth, Kde};

            const KDE_POINTS: usize = 100;
            const SAMPLE_SIZE: usize = 100_000;

            #[bench]
            fn call(b: &mut Bencher) {
                let data = ::bench::vec::<$ty>();
                let kde = Kde::new(Sample::new(&data), Gaussian, Bandwidth::Silverman);
                let x = Sample::new(&data).mean();

                b.iter(|| {
                    kde(x)
                })
            }

            #[bench]
            fn map(b: &mut Bencher) {
                let data = ::test::vec(SAMPLE_SIZE, 0).unwrap();
                let kde = Kde::new(Sample::new(&data), Gaussian, Bandwidth::Silverman);
                let xs: Vec<_> = ::space::linspace::<$ty>(0., 1., KDE_POINTS).collect();

                b.iter(|| {
                    kde.map(&xs)
                })
            }
        }
    }
}

#[cfg(test)]
mod bench {
    bench!(f32);
    bench!(f64);
}
