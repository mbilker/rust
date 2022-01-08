// run-pass

pub trait Foo {
  const N: usize;
}

pub struct Bar;

impl Foo for Bar {
  const N: usize = 3;
}

const TEST:usize = 3;


fn foo<F: Foo<N=3>>() {}
fn bar<F: Foo<N={TEST}>>() {}

fn main() {}
