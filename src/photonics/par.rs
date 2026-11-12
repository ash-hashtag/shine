//! Data parallelism that falls back to sequential code where threads are
//! unavailable (WebAssembly in the browser).

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// How many threads [`map`] runs on.
pub fn threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rayon::current_num_threads()
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// `items.map(f)` in parallel when possible, preserving order.
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        items.par_iter().map(f).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.iter().map(f).collect()
    }
}

/// Like [`map`], stopping at an error.
pub fn try_map<T: Sync, R: Send, E: Send>(
    items: &[T],
    f: impl Fn(&T) -> Result<R, E> + Sync + Send,
) -> Result<Vec<R>, E> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        items.par_iter().map(f).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        items.iter().map(f).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_order_and_errors() {
        let items: Vec<u32> = (0..1000).collect();
        assert_eq!(
            map(&items, |x| x * 2),
            items.iter().map(|x| x * 2).collect::<Vec<_>>()
        );
        assert_eq!(try_map(&items, |&x| Ok::<_, ()>(x + 1)).unwrap()[999], 1000);
        assert_eq!(
            try_map(&items, |&x| if x == 500 { Err("boom") } else { Ok(x) }),
            Err("boom")
        );
        assert!(map(&[] as &[u32], |x| *x).is_empty());
    }
}
