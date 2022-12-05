use neon::prelude::*;

use neon::handle::Managed;
use neon::types::function::CallOptions;

use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;

pub type CallbackHandle = (Channel, Arc<Root<JsFunction>>);

pub struct CallbackList {
    list: Mutex<Vec<CallbackHandle>>
}

impl CallbackList {
    pub fn extract(&self) -> Vec<CallbackHandle> {
        if let Ok(ref mut list) = self.list.lock() {
            return list.clone();
        }

        Vec::new()
    }

    pub const fn new() -> Self {
        Self {
            list: Mutex::new(Vec::new())
        }
    }

    pub fn add(&self, cx: &mut FunctionContext, f: Handle<JsFunction>) -> usize {
        let tx = cx.channel();
        let root = Arc::new(f.root(cx));
        if let Ok(ref mut list) = self.list.lock() {
            list.push((tx, root.clone()));
        }

        Arc::as_ptr(&root) as usize
    }

    pub fn remove(&self, ptr: usize) {
        if let Ok(ref mut list) = self.list.lock() {
            list.retain(|(_, x)| Arc::as_ptr(&x) as usize != ptr);
        }
    }

    pub fn clear(&self) {
        if let Ok(ref mut list) = self.list.lock() {
            list.clear();
        }
    }

    pub fn notify(&self, value: &str) {
        let name = String::from(value);
        self.notify_with(move |cx, opts| opts.arg(cx.string(name.clone())));
    }

    pub fn notify_with<F>(&self, factory: F) where F: for<'a, 'b> Fn(&mut TaskContext<'a>, &'b mut CallOptions<'a>) -> &'b mut CallOptions<'a> + Sync + Send + 'static {
        let arc = Arc::new(factory);
        for (tx, root) in self.extract() {
            let f = arc.clone();
            tx.send(move |mut cx| {
                let cb = (*root).to_inner(&mut cx);
                let mut opts = cb.call_with(&mut cx);
                (*f)(&mut cx, &mut opts).exec(&mut cx)?;

                Ok(())
            });
        }
    }
}

pub struct Event {
    mx: Mutex<Option<bool>>,
    sem: Condvar,
}

impl Event {
    pub const fn new() -> Self {
        Self {
            mx: Mutex::new(None),
            sem: Condvar::new(),
        }
    }

    pub fn set(&self, value: bool) -> bool {
        if let Ok(mut state) = self.mx.lock() {
            if (*state).is_some() {
                return false;
            }

            *state = Some(value);
        }

        self.sem.notify_all();
        return true;
    }

    pub fn wait(&self) -> bool {
        if let Ok(state) = self.mx.lock() {
            if let Some(result) = *state {
                return result;
            }

            if let Ok(after) = self.sem.wait(state) {
                if let Some(result) = *after {
                    return result;
                }    
            }
        }

        return false;
    }
}

pub struct JsArc<T>(Arc<T>);
pub type JsArcResult<'a, T> = JsResult<'a, JsBox<JsArc<T>>>;

impl<T> JsArc<T> where T: Send + 'static {
    pub fn export<'a>(cx: &mut FunctionContext<'a>, value: T) -> JsArcResult<'a, T> {
        Ok(cx.boxed(Self(Arc::new(value))))
    }

    pub fn import(cx: &mut FunctionContext, i: i32) -> NeonResult<Arc<T>> {
        let h = cx.argument::<JsBox<JsArc<T>>>(i)?;
        Ok((**h).0.clone())
    }
}

impl<T> Finalize for JsArc<T> {

}

unsafe impl<T> Send for JsArc<T> {

}

pub fn opt_arg_at<'a, T: Managed + Value>(cx: &mut FunctionContext<'a>, i: i32) -> NeonResult<Option<Handle<'a, T>>> {
    if let Some(arg) = cx.argument_opt(i) {
        if arg.downcast::<JsUndefined, FunctionContext>(cx).is_err() {
            return Ok(Some(arg.downcast_or_throw::<T, FunctionContext>(cx)?));
        }
    }

    Ok(None)
}