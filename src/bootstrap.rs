use parallel;
//use std::{cmp, comm, mem, os, ptr, raw};
use std::{os, ptr};

use {Bootstrap, Distribution};
use resamples::Resamples;

impl<T: Clone + Sync> Bootstrap for [T] {
    fn bootstrap<A: Send>(&self, statistic: fn(&[T]) -> A, nresamples: uint) -> Distribution<A> {
        // FIXME `RUST_THREADS` should be favored over `num_cpus`
        let ncpus = os::num_cpus();

        // TODO Under what conditions should multi thread by favored?
        if ncpus > 1 && nresamples > self.len() {
            let granularity = nresamples / ncpus + 1;
            let mut distribution = Vec::with_capacity(nresamples);
            unsafe { distribution.set_len(nresamples) }

            parallel::divide(distribution[mut], granularity, |data, _| {
                let mut resamples = Resamples::new(self);

                for ptr in data.iter_mut() {
                    unsafe { ptr::write(ptr, statistic(resamples.next())) }
                }
            });

            Distribution(distribution)
        } else {
            let mut resamples = Resamples::new(self);

            Distribution(range(0, nresamples).map(|_| {
                statistic(resamples.next())
            }).collect())
        }
    }
}

/// Returns the bootstrap distribution of the parameter estimated by the 2-sample statistic
///
/// * Bootstrap method: Case resampling
#[experimental]
pub fn bootstrap<A: Clone + Sync, B: Clone + Sync, C: Send>(
    first: &[A],
    second: &[B],
    statistic: fn(&[A], &[B]) -> C,
    nresamples: uint
) -> Distribution<C> {
    assert!(nresamples > 0);

    // FIXME `RUST_THREADS` should be favored over `num_cpus`
    let ncpus = os::num_cpus();
    let nresamples_sqrt = (nresamples as f64).sqrt().ceil() as uint;
    let nresamples = nresamples_sqrt * nresamples_sqrt;

    // TODO Under what conditions should multi thread by favored?
    if ncpus > 1 && nresamples > first.len() + second.len() {
        let granularity = nresamples_sqrt / ncpus + 1;
        let mut distribution = Vec::with_capacity(nresamples);
        unsafe { distribution.set_len(nresamples) }

        parallel::divide(distribution[mut], granularity, |data, _| {
            let mut resamples = Resamples::new(first);
            let mut other_resamples = Resamples::new(second);

            for chunk in data.chunks_mut(granularity) {
                let resample = resamples.next();

                for ptr in chunk.iter_mut() {
                    let other_resample = other_resamples.next();

                    unsafe { ptr::write(ptr, statistic(resample, other_resample)) }
                }
            }
        });

        Distribution(distribution)
    } else {
        let mut resamples = Resamples::new(first);
        let mut other_resamples = Resamples::new(second);
        let mut distribution = Vec::with_capacity(nresamples);

        for _ in range(0, nresamples_sqrt) {
            let resample = resamples.next();

            for _ in range(0, nresamples_sqrt) {
                let other_resample = other_resamples.next();

                distribution.push(statistic(resample, other_resample));
            }
        }

        Distribution(distribution)
    }
}

#[cfg(test)]
mod test {
    use quickcheck::TestResult;

    use {Bootstrap, Stats};
    use test;

    #[quickcheck]
    fn bootstrap(size: uint, nresamples: uint) -> TestResult {
        fn mean(sample: &[f64]) -> f64 {
            sample.mean()
        }

        if let Some(sample) = test::vec::<f64>(size) {
            let distribution = if nresamples > 0 {
                sample[].bootstrap(mean, nresamples).unwrap()
            } else {
                return TestResult::discard();
            };

            TestResult::from_bool(
                // Allocated memory in the most efficient way
                distribution.capacity() == distribution.len() &&
                // Computed the correct number of resamples
                distribution.len() == nresamples &&
                // No uninitialized values
                distribution.iter().all(|&x| x >= 0. && x <= 1.)
            )
        } else {
            TestResult::discard()
        }
    }

    #[quickcheck]
    fn bootstrap2((size, another_size): (uint, uint), nresamples: uint) -> TestResult {
        if let (Some(first), Some(second)) =
            (test::vec::<f64>(size), test::vec::<f64>(another_size))
        {
            let distribution = if nresamples > 0 {
                super::bootstrap(first[], second[], ::t, nresamples).unwrap()
            } else {
                return TestResult::discard();
            };

            let nresamples_sqrt = (nresamples as f64).sqrt().ceil() as uint;
            let nresamples = nresamples_sqrt * nresamples_sqrt;

            TestResult::from_bool(
                // Allocated memory in the most efficient way
                distribution.capacity() == distribution.len() &&
                // Computed the correct number of resamples
                distribution.len() == nresamples
            )
        } else {
            TestResult::discard()
        }

    }
}

#[cfg(test)]
mod bench {
    use std_test::Bencher;

    use {Bootstrap, Stats};
    use regression::{Slope, StraightLine};
    use test;

    static NRESAMPLES: uint = 100_000;
    static SAMPLE_SIZE: uint = 100;

    #[bench]
    fn bootstrap_mean(b: &mut Bencher) {
        fn mean(sample: &[f64]) -> f64 {
            sample.mean()
        }

        let sample = test::vec::<f64>(SAMPLE_SIZE).unwrap();

        b.iter(|| {
            sample[].bootstrap(mean, NRESAMPLES)
        });
    }

    #[bench]
    fn bootstrap_sl(b: &mut Bencher) {
        fn slr(sample: &[(f64, f64)]) -> StraightLine<f64> {
            StraightLine::fit(sample)
        }

        let sample = test::vec::<(f64, f64)>(SAMPLE_SIZE).unwrap();

        b.iter(|| {
            sample[].bootstrap(slr, NRESAMPLES)
        })
    }

    #[bench]
    fn bootstrap_slope(b: &mut Bencher) {
        fn slr(sample: &[(f64, f64)]) -> Slope<f64> {
            Slope::fit(sample)
        }

        let sample = test::vec::<(f64, f64)>(SAMPLE_SIZE).unwrap();

        b.iter(|| {
            sample[].bootstrap(slr, NRESAMPLES)
        })
    }
}