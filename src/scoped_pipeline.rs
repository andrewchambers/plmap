#[rustversion::since(1.63)]
use {
    super::mapper::Mapper,
    std::{collections::VecDeque, thread},
};

#[rustversion::since(1.63)]
pub struct ScopedPipeline<'scope, 'env, I, M>
where
    I: Iterator,
    <I as Iterator>::Item: Send + 'env,
    M: Mapper<I::Item> + Clone + Send + 'env,
    <M as Mapper<I::Item>>::Out: Send + 'env,
{
    input: I,
    queue: VecDeque<crossbeam_channel::Receiver<M::Out>>,
    dispatch: crossbeam_channel::Sender<(I::Item, crossbeam_channel::Sender<M::Out>)>,
    _worker_scope: &'scope thread::Scope<'scope, 'env>,
    workers: Vec<thread::ScopedJoinHandle<'scope, ()>>,
}

#[rustversion::since(1.63)]
impl<'scope, 'env, I, M> ScopedPipeline<'scope, 'env, I, M>
where
    I: Iterator,
    <I as Iterator>::Item: Send + 'env,
    M: Mapper<I::Item> + Clone + Send + 'env,
    <M as Mapper<I::Item>>::Out: Send + 'env,
{
    pub fn new(
        worker_scope: &'scope thread::Scope<'scope, 'env>,
        n_workers: usize,
        m: M,
        input: I,
    ) -> ScopedPipeline<'scope, 'env, I, M> {
        let n_workers = n_workers.min(1);
        let (dispatch, dispatch_rx): (
            crossbeam_channel::Sender<(I::Item, crossbeam_channel::Sender<M::Out>)>,
            _,
        ) = crossbeam_channel::bounded(0);
        let mut workers = Vec::with_capacity(n_workers);

        for _ in 0..n_workers {
            let mut m = m.clone();
            let dispatch_rx = dispatch_rx.clone();
            let handle = worker_scope.spawn(move || loop {
                match dispatch_rx.recv() {
                    Ok((in_val, respond)) => {
                        let out_val = m.apply(in_val);
                        respond.send(out_val).unwrap();
                    }
                    Err(_) => break,
                }
            });
            workers.push(handle)
        }

        ScopedPipeline {
            input,
            dispatch,
            workers,
            _worker_scope: worker_scope,
            queue: VecDeque::with_capacity(n_workers),
        }
    }
}

#[rustversion::since(1.63)]
impl<'scope, 'env, I, M> Drop for ScopedPipeline<'scope, 'env, I, M>
where
    I: Iterator,
    <I as Iterator>::Item: Send + 'env,
    M: Mapper<I::Item> + Clone + Send + 'env,
    <M as Mapper<I::Item>>::Out: Send + 'env,
{
    fn drop(&mut self) {
        let (dummy, _) = crossbeam_channel::bounded(1);
        self.dispatch = dummy;
        for worker in self.workers.drain(..) {
            worker.join().unwrap();
        }
    }
}

#[rustversion::since(1.63)]
impl<'scope, 'env, I, M> Iterator for ScopedPipeline<'scope, 'env, I, M>
where
    I: Iterator,
    <I as Iterator>::Item: Send + 'env,
    M: Mapper<I::Item> + Clone + Send + 'env,
    <M as Mapper<I::Item>>::Out: Send + 'env,
{
    type Item = <M as Mapper<I::Item>>::Out;

    fn next(&mut self) -> Option<Self::Item> {
        while self.queue.len() < self.workers.len() {
            match self.input.next() {
                Some(v) => {
                    let (tx, rx) = crossbeam_channel::bounded(1);
                    self.queue.push_back(rx);
                    self.dispatch.send((v, tx)).unwrap();
                }
                None => break,
            }
        }

        match self.queue.pop_front() {
            Some(rx) => Some(rx.recv().unwrap()),
            None => None,
        }
    }
}

#[rustversion::since(1.63)]
pub trait ScopedPipelineMap<'scope, 'env, I, M>
where
    I: Iterator,
    <I as Iterator>::Item: Send + 'env,
    M: Mapper<I::Item> + Clone + Send + 'env,
    <M as Mapper<I::Item>>::Out: Send + 'env,
{
    fn scoped_plmap(
        self,
        worker_scope: &'scope thread::Scope<'scope, 'env>,
        n_workers: usize,
        m: M,
    ) -> ScopedPipeline<'scope, 'env, I, M>;
}

#[rustversion::since(1.63)]
impl<'scope, 'env, I, M> ScopedPipelineMap<'scope, 'env, I, M> for I
where
    I: Iterator,
    <I as Iterator>::Item: Send + 'env,
    M: Mapper<I::Item> + Clone + Send + 'env,
    <M as Mapper<I::Item>>::Out: Send + 'env,
{
    fn scoped_plmap(
        self,
        worker_scope: &'scope thread::Scope<'scope, 'env>,
        n_workers: usize,
        m: M,
    ) -> ScopedPipeline<'scope, 'env, I, M> {
        ScopedPipeline::new(worker_scope, n_workers, m, self)
    }
}

#[rustversion::since(1.63)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scoped_parallel_pipeline() {
        thread::scope(|s| {
            for (i, v) in (0..100).scoped_plmap(s, 5, |x| x * 2).enumerate() {
                let i = i as i32;
                assert_eq!(i * 2, v)
            }
            assert_eq!((0..100).scoped_plmap(s, 2, |x| x * 2).count(), 100);
        })
    }
}