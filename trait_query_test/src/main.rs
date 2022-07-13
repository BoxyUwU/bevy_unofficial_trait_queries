use bevy::prelude::*;
use bevy::ptr::{Ptr, PtrMut};
use bevy_unofficial_trait_queries::*;

#[queryable_trait]
trait Foo<const N: usize> {
    fn foo(&self);
}

fn blah(_: Query<&dyn Foo<3>>) {}

fn main() {}
