use bevy::prelude::*;
use bevy_unofficial_trait_queries::{queryable_trait, register_impl};

#[queryable_trait]
trait Foo {
    fn foo(&self);
}

#[derive(Component)]
struct A(u32);
impl Foo for A {
    fn foo(&self) {
        println!("A: {}", self.0);
    }
}

#[derive(Component)]
struct B(u32);
impl Foo for B {
    fn foo(&self) {
        println!("B: {}", self.0);
    }
}

fn startup(mut cmds: Commands<'_, '_>) {
    dbg!(cmds.spawn().insert(A(12)).id());
    dbg!(cmds.spawn().insert(A(4)).insert(B(8)).id());
    dbg!(cmds.spawn().insert(B(1)).insert(A(28)).id());
    dbg!(cmds.spawn().id());
}

fn blah(mut query: Query<(Entity, &dyn Foo)>) {
    for (e, trait_objects) in query.iter_mut() {
        println!("Entity: {:?}", e);
        for obj in &trait_objects {
            obj.foo();
        }
        println!("no more");
    }
    std::process::exit(0);
}

fn main() {
    let mut app = App::new();
    register_impl::<A, dyn Foo>(&mut app);
    register_impl::<B, dyn Foo>(&mut app);
    app.add_startup_system(startup).add_system(blah).run();
}
