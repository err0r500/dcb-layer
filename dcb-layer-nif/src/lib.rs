use rustler::{Env, Term};

mod atoms {
    rustler::atoms! {
        ok,
        error,
    }
}

fn load(_env: Env, _info: Term) -> bool {
    true
}

rustler::init!("Elixir.Dcb.Native", load = load);
