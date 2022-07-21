// FIXME almost certainly a `Query<&mut dyn Trait> does not conflict with a `ResMut<TraitImplsFor<Trait>>` which is unsound lol

pub use bevy_unofficial_trait_queries_macros::queryable_trait;

use std::{any::TypeId, cell::UnsafeCell, marker::PhantomData, ptr::NonNull};

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

pub fn register_impl<T: Component, Trait: TraitQueryArg + ?Sized>(app: &mut App)
where
    Trait: SynthesiseMetaFrom<T>,
{
    let id = app.world.init_component::<T>();
    let meta = <Trait as SynthesiseMetaFrom<T>>::make_meta();
    let mut impl_registry = app
        .world
        .get_resource_or_insert_with::<TraitImplsFor<Trait>>(|| TraitImplsFor::<Trait> {
            ids: vec![],
            metas: vec![],
            _p: PhantomData,
        });
    impl_registry.ids.push(id);
    impl_registry.metas.push(meta);
}

pub struct Mut<'w, T: ?Sized> {
    component_ticks: &'w mut ComponentTicks,
    last_change_tick: u32,
    change_tick: u32,
    ptr: &'w mut T,
}
impl<'w, T: ?Sized> Mut<'w, T> {
    #[doc(hidden)]
    pub unsafe fn __new_mut(
        component_ticks: &'w mut ComponentTicks,
        last_change_tick: u32,
        change_tick: u32,
        ptr: &'w mut T,
    ) -> Mut<'w, T> {
        Mut {
            component_ticks,
            last_change_tick,
            change_tick,
            ptr,
        }
    }

    #[inline]
    pub fn is_added(&self) -> bool {
        self.component_ticks
            .is_added(self.last_change_tick, self.change_tick)
    }

    #[inline]
    pub fn is_changed(&self) -> bool {
        self.component_ticks
            .is_changed(self.last_change_tick, self.change_tick)
    }

    #[inline]
    pub fn set_changed(&mut self) {
        self.component_ticks.set_changed(self.change_tick);
    }

    #[inline]
    pub fn last_changed(&self) -> u32 {
        self.last_change_tick
    }
}
impl<'w, T: ?Sized> std::ops::Deref for Mut<'w, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.ptr
    }
}
impl<'w, T: ?Sized> std::ops::DerefMut for Mut<'w, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.set_changed();
        &mut self.ptr
    }
}

struct TraitImplsFor<T: TraitQueryArg + ?Sized> {
    ids: Vec<ComponentId>,
    metas: Vec<T::Meta>,
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
        last_change_tick: u32,
        change_tick: u32,
        meta: &'a Self::Meta,
    ) -> <Self as TraitQueryArgGats<'a>>::ItemMut;
}

/// SAFETY: it must be sound to transmute `<_ as TraitQueryArgGats<'a>>::Item/Mut` to `<_ as TraitQueryArgGats<'b>>::Item/Mut`
/// if `'a: 'b` holds.
pub unsafe trait TraitQueryArgGats<'a>: 'static {
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

pub struct DynTraitReadQueryItem<'w, Trait: TraitQueryArg + ?Sized> {
    metas: &'w [Trait::Meta],
    ptrs: Vec<Option<Ptr<'w>>>,
}
pub struct DynTraitReadQueryItemIter<'a, Trait: TraitQueryArg + ?Sized> {
    metas: &'a [Trait::Meta],
    ptrs: &'a [Option<Ptr<'a>>],
}
impl<'a, Trait: TraitQueryArg + ?Sized> Iterator for DynTraitReadQueryItemIter<'a, Trait> {
    type Item = <Trait as TraitQueryArgGats<'a>>::Item;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (first_meta, rest_metas) = self.metas.split_first()?;
            let (first_ptr, rest_ptrs) = self.ptrs.split_first().unwrap();
            self.metas = rest_metas;
            self.ptrs = rest_ptrs;
            let ptr = match first_ptr {
                None => continue,
                Some(ptr) => ptr,
            };
            break Some(unsafe { Trait::make_item(*ptr, first_meta) });
        }
    }
}
impl<'a, Trait: TraitQueryArg + ?Sized> IntoIterator for &'a DynTraitReadQueryItem<'_, Trait> {
    type IntoIter = DynTraitReadQueryItemIter<'a, Trait>;
    type Item = <Trait as TraitQueryArgGats<'a>>::Item;
    fn into_iter(self) -> Self::IntoIter {
        DynTraitReadQueryItemIter {
            metas: self.metas,
            ptrs: self.ptrs.as_slice(),
        }
    }
}

pub struct DynTraitReadQuery<Trait: ?Sized>(PhantomData<fn() -> Box<Trait>>);

#[doc(hidden)]
pub struct DynTraitRWFetch<'w, T: TraitQueryArg + ?Sized> {
    metas: &'w [T::Meta],
    fetches: Vec<(Option<DynRWFetch<'w>>, bool)>,
    last_change_tick: u32,
    change_tick: u32,
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
        item
    }

    unsafe fn init_fetch<'w>(
        world: &'w World,
        state: &DynTraitRWState,
        last_change_tick: u32,
        change_tick: u32,
    ) -> <Self as WorldQueryGats<'w>>::Fetch {
        DynTraitRWFetch {
            metas: world
                .get_resource::<TraitImplsFor<Trait>>()
                .unwrap()
                .metas
                .as_slice(),
            fetches: state
                .states
                .iter()
                .map(|state| {
                    (
                        DynRead::init_fetch(world, state, last_change_tick, change_tick),
                        false,
                    )
                })
                .collect::<Vec<(Option<DynRWFetch<'w>>, bool)>>(),
            last_change_tick,
            change_tick,
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
        DynTraitReadQueryItem {
            ptrs: fetch
                .fetches
                .iter_mut()
                .map(|(fetch, matches)| match matches {
                    false => None,
                    true => Some(DynRead::archetype_fetch(fetch, archetype_index)),
                })
                .collect::<Vec<_>>(),
            metas: &fetch.metas,
        }
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
                .get_resource_id(TypeId::of::<TraitImplsFor<Trait>>())
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

pub struct DynTraitWriteQueryItem<'w, Trait: TraitQueryArg + ?Sized> {
    metas: &'w [Trait::Meta],
    ptrs: Vec<Option<(PtrMut<'w>, &'w UnsafeCell<ComponentTicks>)>>,
    last_change_tick: u32,
    change_tick: u32,
}
pub struct DynTraitWriteQueryItemIterMut<'a, 'w, Trait: TraitQueryArg + ?Sized> {
    metas: &'a [Trait::Meta],
    ptrs: &'a mut [Option<(PtrMut<'w>, &'w UnsafeCell<ComponentTicks>)>],
    last_change_tick: u32,
    change_tick: u32,
}
impl<'a, Trait: TraitQueryArg + ?Sized> Iterator for DynTraitWriteQueryItemIterMut<'a, '_, Trait> {
    type Item = <Trait as TraitQueryArgGats<'a>>::ItemMut;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (first_meta, rest_metas) = self.metas.split_first()?;
            let ptrs = std::mem::replace(&mut self.ptrs, &mut []);
            let (first_ptr, rest_ptrs) = ptrs.split_first_mut().unwrap();
            self.metas = rest_metas;
            self.ptrs = rest_ptrs;
            let (ptr, ticks) = match first_ptr {
                None => continue,
                Some(ptr) => ptr,
            };
            break Some(unsafe {
                Trait::make_item_mut(
                    // FIXME `PtrMut::reborrow_mut` would be a good idea
                    PtrMut::new(NonNull::new(ptr.as_ptr()).unwrap()),
                    ticks,
                    self.last_change_tick,
                    self.change_tick,
                    first_meta,
                )
            });
        }
    }
}
impl<'a, 'w, Trait: TraitQueryArg + ?Sized> IntoIterator
    for &'a mut DynTraitWriteQueryItem<'w, Trait>
{
    type IntoIter = DynTraitWriteQueryItemIterMut<'a, 'w, Trait>;
    type Item = <Trait as TraitQueryArgGats<'a>>::ItemMut;
    fn into_iter(self) -> Self::IntoIter {
        DynTraitWriteQueryItemIterMut {
            metas: self.metas,
            ptrs: self.ptrs.as_mut_slice(),
            last_change_tick: self.last_change_tick,
            change_tick: self.change_tick,
        }
    }
}
pub struct DynTraitWriteQueryItemIter<'a, Trait: TraitQueryArg + ?Sized> {
    metas: &'a [Trait::Meta],
    ptrs: &'a [Option<(PtrMut<'a>, &'a UnsafeCell<ComponentTicks>)>],
}
impl<'a, Trait: TraitQueryArg + ?Sized> Iterator for DynTraitWriteQueryItemIter<'a, Trait> {
    type Item = <Trait as TraitQueryArgGats<'a>>::Item;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (first_meta, rest_metas) = self.metas.split_first()?;
            let (first_ptr, rest_ptrs) = self.ptrs.split_first().unwrap();
            self.metas = rest_metas;
            self.ptrs = rest_ptrs;
            let (ptr, _) = match first_ptr {
                None => continue,
                Some(ptr) => ptr,
            };
            break Some(unsafe {
                // FIXME `PtrMut::reborrow` would be good
                Trait::make_item(Ptr::new(NonNull::new(ptr.as_ptr()).unwrap()), first_meta)
            });
        }
    }
}
impl<'a, Trait: TraitQueryArg + ?Sized> IntoIterator for &'a DynTraitWriteQueryItem<'_, Trait> {
    type IntoIter = DynTraitWriteQueryItemIter<'a, Trait>;
    type Item = <Trait as TraitQueryArgGats<'a>>::Item;
    fn into_iter(self) -> Self::IntoIter {
        DynTraitWriteQueryItemIter {
            metas: self.metas,
            ptrs: self.ptrs.as_slice(),
        }
    }
}
pub struct DynTraitWriteQueryItemIntoIter<'w, Trait: TraitQueryArg + ?Sized> {
    metas: std::slice::Iter<'w, Trait::Meta>,
    ptrs: std::vec::IntoIter<Option<(PtrMut<'w>, &'w UnsafeCell<ComponentTicks>)>>,
    last_change_tick: u32,
    change_tick: u32,
}
impl<'w, Trait: TraitQueryArg + ?Sized> Iterator for DynTraitWriteQueryItemIntoIter<'w, Trait> {
    type Item = <Trait as TraitQueryArgGats<'w>>::ItemMut;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let meta = self.metas.next()?;
            let (ptr, ticks) = match self.ptrs.next().unwrap() {
                Some(ptr) => ptr,
                None => continue,
            };
            break Some(unsafe {
                Trait::make_item_mut(ptr, ticks, self.last_change_tick, self.change_tick, meta)
            });
        }
    }
}
impl<'w, Trait: TraitQueryArg + ?Sized> IntoIterator for DynTraitWriteQueryItem<'w, Trait> {
    type Item = <Trait as TraitQueryArgGats<'w>>::ItemMut;
    type IntoIter = DynTraitWriteQueryItemIntoIter<'w, Trait>;
    fn into_iter(self) -> Self::IntoIter {
        DynTraitWriteQueryItemIntoIter {
            metas: self.metas.into_iter(),
            ptrs: self.ptrs.into_iter(),
            last_change_tick: self.last_change_tick,
            change_tick: self.change_tick,
        }
    }
}

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
        item
    }

    unsafe fn init_fetch<'w>(
        world: &'w World,
        state: &DynTraitRWState,
        last_change_tick: u32,
        change_tick: u32,
    ) -> <Self as WorldQueryGats<'w>>::Fetch {
        DynTraitRWFetch {
            metas: world
                .get_resource::<TraitImplsFor<Trait>>()
                .unwrap()
                .metas
                .as_slice(),
            fetches: state
                .states
                .iter()
                .map(|state| {
                    (
                        DynWrite::init_fetch(world, state, last_change_tick, change_tick),
                        false,
                    )
                })
                .collect::<Vec<(Option<DynRWFetch<'w>>, bool)>>(),
            last_change_tick,
            change_tick,
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
        DynTraitWriteQueryItem {
            ptrs: fetch
                .fetches
                .iter_mut()
                .map(|(fetch, matches)| match matches {
                    false => None,
                    true => Some(DynWrite::archetype_fetch(fetch, archetype_index)),
                })
                .collect::<Vec<_>>(),
            metas: &fetch.metas,
            last_change_tick: fetch.last_change_tick,
            change_tick: fetch.change_tick,
        }
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
                .get_resource_id(TypeId::of::<TraitImplsFor<Trait>>())
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
