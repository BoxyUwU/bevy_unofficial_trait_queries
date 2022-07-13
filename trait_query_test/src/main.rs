use bevy_unofficial_trait_queries::queryable_trait;

#[queryable_trait]
trait Foo<const N: usize> {
    fn foo(&self);
}

fn blah(_: bevy::prelude::Query<&mut dyn Foo<3>>) {}

fn main() {}
