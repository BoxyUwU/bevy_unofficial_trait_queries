pub use bevy_unofficial_trait_queries_macros::queryable_trait;

use std::{any::TypeId, cell::UnsafeCell, marker::PhantomData};

use bevy::{
    ecs::{
        archetype::{Archetype, ArchetypeComponentId},
        component::{ComponentId, ComponentTicks, StorageType},
        query::{Access, FilteredAccess, ReadOnlyWorldQuery, WorldQuery, WorldQueryGats},
        storage::{Column, ComponentSparseSet},
    },
    prelude::*,
    ptr::{Ptr, PtrMut},
};

struct TraitImplsFor<T: TraitQueryArg + ?Sized> {
    ids: Vec<ComponentId>,
    meta: Vec<T::Meta>,
    _p: PhantomData<fn() -> T>,
}

pub trait TraitQueryArg: for<'a> TraitQueryArgGats<'a> {
    type Meta: Send + Sync + 'static;
    unsafe fn make_item<'a>(
        ptr: Ptr<'a>,
        meta: &'a Self::Meta,
    ) -> <Self as TraitQueryArgGats<'a>>::Item;
    unsafe fn make_item_mut<'a>(
        ptr: PtrMut<'a>,
        ticks: &'a UnsafeCell<ComponentTicks>,
        meta: &'a Self::Meta,
    ) -> <Self as TraitQueryArgGats<'a>>::ItemMut;
}

pub trait TraitQueryArgGats<'a>: 'static {
    type Item;
    type ItemMut;
}

/// `impl<T: Trait> SynthesiseMetaFrom<T> for dyn Trait`
pub trait SynthesiseMetaFrom<T>: TraitQueryArg {
    fn make_meta() -> Self::Meta;
}

//
//

struct DynRead;
enum DynRWFetch<'w> {
    Table {
        column: &'w Column,
        table_rows: &'w [usize],
    },
    SparseSet {
        entities: &'w [Entity],
        sparse_set: &'w ComponentSparseSet,
    },
}

#[derive(Copy, Clone)]
struct DynRWState {
    id: ComponentId,
    storage: StorageType,
}

impl<'a> WorldQueryGats<'a> for DynRead {
    type Item = Ptr<'a>;
    type Fetch = Option<DynRWFetch<'a>>;
}
impl DynRead {
    fn shrink<'wlong: 'wshort, 'wshort>(
        item: <Self as WorldQueryGats<'wlong>>::Item,
    ) -> <Self as WorldQueryGats<'wshort>>::Item {
        item
    }

    fn init_state(world: &mut World, id: ComponentId) -> DynRWState {
        DynRWState {
            id,
            storage: world.components().get_info(id).unwrap().storage_type(),
        }
    }

    unsafe fn init_fetch<'w>(
        world: &'w World,
        state: &DynRWState,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> <Self as WorldQueryGats<'w>>::Fetch {
        match state.storage {
            StorageType::SparseSet => Some(DynRWFetch::SparseSet {
                entities: &[],
                sparse_set: world.storages().sparse_sets.get(state.id).unwrap(),
            }),
            StorageType::Table => None,
        }
    }

    unsafe fn set_archetype<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        state: &DynRWState,
        archetype: &'w Archetype,
        tables: &'w bevy::ecs::storage::Tables,
    ) {
        match state.storage {
            StorageType::SparseSet => match fetch.as_mut().unwrap() {
                DynRWFetch::Table { .. } => unreachable!(),
                DynRWFetch::SparseSet { entities, .. } => *entities = archetype.entities(),
            },
            StorageType::Table => {
                *fetch = Some(DynRWFetch::Table {
                    column: tables
                        .get(archetype.table_id())
                        .unwrap()
                        .get_column(state.id)
                        .unwrap(),
                    table_rows: archetype.entity_table_rows(),
                })
            }
        }
    }

    unsafe fn set_table<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _state: &DynRWState,
        _table: &'w bevy::ecs::storage::Table,
    ) {
        unreachable!()
    }

    unsafe fn archetype_fetch<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        archetype_index: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        match fetch.as_mut().unwrap() {
            DynRWFetch::Table { column, table_rows } => {
                let column_idx = table_rows[archetype_index];
                column.get_data_unchecked(column_idx)
            }
            DynRWFetch::SparseSet {
                entities,
                sparse_set,
            } => {
                let entity = *entities.get(archetype_index).unwrap();
                sparse_set.get(entity).unwrap()
            }
        }
    }

    unsafe fn table_fetch<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _table_row: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        unreachable!()
    }

    fn update_component_access(state: &DynRWState, access: &mut FilteredAccess<ComponentId>) {
        access.add_read(state.id);
    }

    fn update_archetype_component_access(
        state: &DynRWState,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        access.add_read(archetype.get_archetype_component_id(state.id).unwrap());
    }

    fn matches_component_set(
        state: &DynRWState,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        set_contains_id(state.id)
    }
}

struct DynWrite;
impl<'a> WorldQueryGats<'a> for DynWrite {
    type Item = (PtrMut<'a>, &'a UnsafeCell<ComponentTicks>);
    type Fetch = Option<DynRWFetch<'a>>;
}
impl DynWrite {
    fn shrink<'wlong: 'wshort, 'wshort>(
        item: <Self as WorldQueryGats<'wlong>>::Item,
    ) -> <Self as WorldQueryGats<'wshort>>::Item {
        item
    }

    fn init_state(world: &mut World, id: ComponentId) -> DynRWState {
        DynRWState {
            id,
            storage: world.components().get_info(id).unwrap().storage_type(),
        }
    }

    unsafe fn init_fetch<'w>(
        world: &'w World,
        state: &DynRWState,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> <Self as WorldQueryGats<'w>>::Fetch {
        match state.storage {
            StorageType::SparseSet => Some(DynRWFetch::SparseSet {
                entities: &[],
                sparse_set: world.storages().sparse_sets.get(state.id).unwrap(),
            }),
            StorageType::Table => None,
        }
    }

    unsafe fn set_archetype<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        state: &DynRWState,
        archetype: &'w Archetype,
        tables: &'w bevy::ecs::storage::Tables,
    ) {
        match state.storage {
            StorageType::SparseSet => match fetch.as_mut().unwrap() {
                DynRWFetch::Table { .. } => unreachable!(),
                DynRWFetch::SparseSet { entities, .. } => *entities = archetype.entities(),
            },
            StorageType::Table => {
                *fetch = Some(DynRWFetch::Table {
                    column: tables
                        .get(archetype.table_id())
                        .unwrap()
                        .get_column(state.id)
                        .unwrap(),
                    table_rows: archetype.entity_table_rows(),
                })
            }
        }
    }

    unsafe fn set_table<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _state: &DynRWState,
        _table: &'w bevy::ecs::storage::Table,
    ) {
        unreachable!()
    }

    unsafe fn archetype_fetch<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        archetype_index: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        match fetch.as_mut().unwrap() {
            DynRWFetch::Table { column, table_rows } => {
                let column_idx = table_rows[archetype_index];
                (
                    column.get_data_unchecked(column_idx).assert_unique(),
                    column.get_ticks_unchecked(column_idx),
                )
            }
            DynRWFetch::SparseSet {
                entities,
                sparse_set,
            } => {
                let entity = *entities.get(archetype_index).unwrap();
                (
                    sparse_set.get(entity).unwrap().assert_unique(),
                    sparse_set.get_ticks(entity).unwrap(),
                )
            }
        }
    }

    unsafe fn table_fetch<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _table_row: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        unreachable!()
    }

    fn update_component_access(state: &DynRWState, access: &mut FilteredAccess<ComponentId>) {
        access.add_write(state.id);
    }

    fn update_archetype_component_access(
        state: &DynRWState,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        access.add_write(archetype.get_archetype_component_id(state.id).unwrap());
    }

    fn matches_component_set(
        state: &DynRWState,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        set_contains_id(state.id)
    }
}
//
//

pub struct DynTraitReadQueryItem<'w, Trait: TraitQueryArg + ?Sized>(
    Vec<<Trait as TraitQueryArgGats<'w>>::Item>,
);

pub struct DynTraitReadQuery<Trait: ?Sized>(PhantomData<fn() -> Box<Trait>>);

#[doc(hidden)]
pub struct DynTraitRWFetch<'w, T: TraitQueryArg + ?Sized> {
    metas: &'w [T::Meta],
    fetches: Vec<(Option<DynRWFetch<'w>>, bool)>,
}
#[doc(hidden)]
pub struct DynTraitRWState {
    resource_id: ComponentId,
    states: Vec<DynRWState>,
}

unsafe impl<Trait: TraitQueryArg + ?Sized + 'static> ReadOnlyWorldQuery
    for DynTraitReadQuery<Trait>
{
}

impl<'a, Trait: TraitQueryArg + ?Sized + 'static> WorldQueryGats<'a> for DynTraitReadQuery<Trait> {
    type Item = DynTraitReadQueryItem<'a, Trait>;
    type Fetch = DynTraitRWFetch<'a, Trait>;
}
unsafe impl<Trait: TraitQueryArg + ?Sized + 'static> WorldQuery for DynTraitReadQuery<Trait> {
    type ReadOnly = Self;

    type State = DynTraitRWState;

    fn shrink<'wlong: 'wshort, 'wshort>(
        item: <Self as WorldQueryGats<'wlong>>::Item,
    ) -> <Self as WorldQueryGats<'wshort>>::Item {
        todo!()
    }

    unsafe fn init_fetch<'w>(
        world: &'w World,
        state: &DynTraitRWState,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> <Self as WorldQueryGats<'w>>::Fetch {
        DynTraitRWFetch {
            metas: world
                .get_resource::<TraitImplsFor<Trait>>()
                .unwrap()
                .meta
                .as_slice(),
            fetches: state
                .states
                .iter()
                .map(|state| {
                    (
                        DynRead::init_fetch(world, state, _last_change_tick, _change_tick),
                        false,
                    )
                })
                .collect::<Vec<(Option<DynRWFetch<'w>>, bool)>>(),
        }
    }

    const IS_DENSE: bool = false;

    const IS_ARCHETYPAL: bool = false;

    unsafe fn set_archetype<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        state: &DynTraitRWState,
        archetype: &'w Archetype,
        tables: &'w bevy::ecs::storage::Tables,
    ) {
        for (fetch, state) in fetch.fetches.iter_mut().zip(state.states.iter()) {
            fetch.1 = DynRead::matches_component_set(state, &|id| archetype.contains(id));
            if fetch.1 {
                DynRead::set_archetype(&mut fetch.0, state, archetype, tables);
            }
        }
    }

    unsafe fn set_table<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _state: &DynTraitRWState,
        _table: &'w bevy::ecs::storage::Table,
    ) {
        unreachable!()
    }

    unsafe fn archetype_fetch<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        archetype_index: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        DynTraitReadQueryItem(
            fetch
                .fetches
                .iter_mut()
                .zip(fetch.metas.iter())
                .flat_map(|((fetch, matches), meta)| match matches {
                    false => None,
                    true => Some(Trait::make_item(
                        DynRead::archetype_fetch(fetch, archetype_index),
                        meta,
                    )),
                })
                .collect::<Vec<_>>(),
        )
    }

    unsafe fn table_fetch<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _table_row: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        unreachable!()
    }

    fn update_component_access(state: &DynTraitRWState, access: &mut FilteredAccess<ComponentId>) {
        access.access_mut().add_read(state.resource_id);
        for state in state.states.iter() {
            access.access_mut().add_read(state.id);
        }
    }

    fn update_archetype_component_access(
        state: &DynTraitRWState,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        for state in state.states.iter() {
            if DynRead::matches_component_set(state, &|id| archetype.contains(id)) {
                DynRead::update_archetype_component_access(state, archetype, access);
            }
        }
    }

    fn init_state(world: &mut World) -> DynTraitRWState {
        DynTraitRWState {
            resource_id: world
                .components()
                .get_resource_id(TypeId::of::<Trait>())
                .unwrap(),
            states: world
                .resource::<TraitImplsFor<Trait>>()
                .ids
                .clone()
                .into_iter()
                .map(|id| DynRead::init_state(world, id))
                .collect::<Vec<DynRWState>>(),
        }
    }

    fn matches_component_set(
        state: &DynTraitRWState,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        state
            .states
            .iter()
            .any(|state| DynRead::matches_component_set(state, set_contains_id))
    }
}

//

pub struct DynTraitWriteQueryItem<'w, Trait: TraitQueryArg + ?Sized>(
    Vec<<Trait as TraitQueryArgGats<'w>>::ItemMut>,
);

pub struct DynTraitWriteQuery<Trait: ?Sized>(PhantomData<fn() -> Box<Trait>>);

impl<'a, Trait: TraitQueryArg + ?Sized + 'static> WorldQueryGats<'a> for DynTraitWriteQuery<Trait> {
    type Item = DynTraitWriteQueryItem<'a, Trait>;
    type Fetch = DynTraitRWFetch<'a, Trait>;
}
unsafe impl<Trait: TraitQueryArg + ?Sized + 'static> WorldQuery for DynTraitWriteQuery<Trait> {
    type ReadOnly = DynTraitReadQuery<Trait>;

    type State = DynTraitRWState;

    fn shrink<'wlong: 'wshort, 'wshort>(
        item: <Self as WorldQueryGats<'wlong>>::Item,
    ) -> <Self as WorldQueryGats<'wshort>>::Item {
        todo!()
    }

    unsafe fn init_fetch<'w>(
        world: &'w World,
        state: &DynTraitRWState,
        _last_change_tick: u32,
        _change_tick: u32,
    ) -> <Self as WorldQueryGats<'w>>::Fetch {
        DynTraitRWFetch {
            metas: world
                .get_resource::<TraitImplsFor<Trait>>()
                .unwrap()
                .meta
                .as_slice(),
            fetches: state
                .states
                .iter()
                .map(|state| {
                    (
                        DynWrite::init_fetch(world, state, _last_change_tick, _change_tick),
                        false,
                    )
                })
                .collect::<Vec<(Option<DynRWFetch<'w>>, bool)>>(),
        }
    }

    const IS_DENSE: bool = false;

    const IS_ARCHETYPAL: bool = false;

    unsafe fn set_archetype<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        state: &DynTraitRWState,
        archetype: &'w Archetype,
        tables: &'w bevy::ecs::storage::Tables,
    ) {
        for (fetch, state) in fetch.fetches.iter_mut().zip(state.states.iter()) {
            fetch.1 = DynWrite::matches_component_set(state, &|id| archetype.contains(id));
            if fetch.1 {
                DynWrite::set_archetype(&mut fetch.0, state, archetype, tables);
            }
        }
    }

    unsafe fn set_table<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _state: &DynTraitRWState,
        _table: &'w bevy::ecs::storage::Table,
    ) {
        unreachable!()
    }

    unsafe fn archetype_fetch<'w>(
        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        archetype_index: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        DynTraitWriteQueryItem(
            fetch
                .fetches
                .iter_mut()
                .zip(fetch.metas.iter())
                .flat_map(|((fetch, matches), meta)| match matches {
                    false => None,
                    true => {
                        let (component_ptr, ticks) =
                            DynWrite::archetype_fetch(fetch, archetype_index);
                        Some(Trait::make_item_mut(component_ptr, ticks, meta))
                    }
                })
                .collect::<Vec<_>>(),
        )
    }

    unsafe fn table_fetch<'w>(
        _fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
        _table_row: usize,
    ) -> <Self as WorldQueryGats<'w>>::Item {
        unreachable!()
    }

    fn update_component_access(state: &DynTraitRWState, access: &mut FilteredAccess<ComponentId>) {
        access.access_mut().add_read(state.resource_id);
        for state in state.states.iter() {
            access.access_mut().add_write(state.id);
        }
    }

    fn update_archetype_component_access(
        state: &DynTraitRWState,
        archetype: &Archetype,
        access: &mut Access<ArchetypeComponentId>,
    ) {
        for state in state.states.iter() {
            if DynWrite::matches_component_set(state, &|id| archetype.contains(id)) {
                DynWrite::update_archetype_component_access(state, archetype, access);
            }
        }
    }

    fn init_state(world: &mut World) -> DynTraitRWState {
        DynTraitRWState {
            resource_id: world
                .components()
                .get_resource_id(TypeId::of::<Trait>())
                .unwrap(),
            states: world
                .resource::<TraitImplsFor<Trait>>()
                .ids
                .clone()
                .into_iter()
                .map(|id| DynWrite::init_state(world, id))
                .collect::<Vec<DynRWState>>(),
        }
    }

    fn matches_component_set(
        state: &DynTraitRWState,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        state
            .states
            .iter()
            .any(|state| DynWrite::matches_component_set(state, set_contains_id))
    }
}