use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{parse_quote, ItemTrait};

extern crate proc_macro;

#[proc_macro_attribute]
pub fn queryable_trait(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = syn::parse::<ItemTrait>(item);
    let mut item = match item {
        Err(_) => {
            return TokenStream::from(quote!(compile_error!(
                "`#[queryable_trait]` should only be used on trait definitions"
            )))
        }
        Ok(item) => item,
    };

    item.supertraits.push(parse_quote!('static));
    let trait_definition = quote!(#item);

    let trait_name = item.ident;
    let (impl_generics, trait_generics, where_clauses) = item.generics.split_for_impl();
    let generics_with_world = {
        let mut generics = item.generics.clone();
        generics.params.insert(0, parse_quote!('__w));
        generics
    };
    let (impl_generics_with_world, _, _) = generics_with_world.split_for_impl();

    let generics_with_t = {
        let mut generics = item.generics.clone();
        generics
            .params
            .push(parse_quote!(__T: #trait_name #trait_generics + 'static));
        generics
    };
    let (impl_generics_with_t, _, _) = generics_with_t.split_for_impl();

    let meta_struct_name = Ident::new(&format!("Dyn{}Meta", trait_name), trait_name.span());

    // i wish this wasnt necessary
    let world_query_impler = |is_readonly: bool| {
        let (deferred_worldquery, readonly_worldquery, opt_mut) = match is_readonly {
            true => (
                quote!(bevy_unofficial_trait_queries::DynTraitReadQuery<dyn #trait_name #trait_generics>),
                quote!(Self),
                quote!(),
            ),
            false => (
                quote!(bevy_unofficial_trait_queries::DynTraitWriteQuery<dyn #trait_name #trait_generics>),
                quote!(&'static (dyn #trait_name #trait_generics + 'static)),
                quote!(mut),
            ),
        };

        // ideally all these impls would be for `&'static dyn Trait<...> + 'static` instead of `&'_ dyn Trait<...> + 'static`
        // but until bevy_ecs changes to `struct Query<Q: WorldQuery + 'static`, a fn sig of `fn foo(_: Query<&dyn Trait>)`
        // is unable to figure out that `&dyn Trait` should be `'static` and ends up requiring manual annotation of `&'static dyn Trait`
        quote! {
            const _: () = {
                use bevy::ecs::{query::*, archetype::{ArchetypeComponentId, Archetype}, prelude::*, component::ComponentId};
                impl #impl_generics_with_world WorldQueryGats<'__w> for &'_ #opt_mut (dyn #trait_name #trait_generics + 'static) {
                    type Item = <#deferred_worldquery as WorldQueryGats<'__w>>::Item;
                    type Fetch = <#deferred_worldquery as WorldQueryGats<'__w>>::Fetch;
                }
                unsafe impl #impl_generics bevy::ecs::query::WorldQuery for &'_ #opt_mut (dyn #trait_name #trait_generics + 'static) {
                    type ReadOnly = #readonly_worldquery;
                    type State = <#deferred_worldquery as WorldQuery>::State;

                    fn shrink<'wlong: 'wshort, 'wshort>(item: bevy::ecs::query::QueryItem<'wlong, Self>) -> bevy::ecs::query::QueryItem<'wshort, Self> {
                        <#deferred_worldquery as WorldQuery>::shrink(item)
                    }

                    unsafe fn init_fetch<'w>(
                        world: &'w World,
                        state: &Self::State,
                        last_change_tick: u32,
                        change_tick: u32,
                    ) -> <Self as WorldQueryGats<'w>>::Fetch {
                        <#deferred_worldquery as WorldQuery>::init_fetch(world, state, last_change_tick, change_tick)
                    }

                    const IS_DENSE: bool = <#deferred_worldquery as WorldQuery>::IS_DENSE;

                    const IS_ARCHETYPAL: bool = <#deferred_worldquery as WorldQuery>::IS_ARCHETYPAL;

                    unsafe fn set_archetype<'w>(
                        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
                        state: &Self::State,
                        archetype: &'w Archetype,
                        tables: &'w bevy::ecs::storage::Tables,
                    ) {
                        <#deferred_worldquery as WorldQuery>::set_archetype(fetch, state, archetype, tables)
                    }

                    unsafe fn set_table<'w>(
                        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
                        state: &Self::State,
                        table: &'w bevy::ecs::storage::Table,
                    ) {
                        <#deferred_worldquery as WorldQuery>::set_table(fetch, state, table)
                    }

                    unsafe fn archetype_fetch<'w>(
                        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
                        archetype_index: usize,
                    ) -> <Self as WorldQueryGats<'w>>::Item {
                        <#deferred_worldquery as WorldQuery>::archetype_fetch(fetch, archetype_index)
                    }

                    unsafe fn table_fetch<'w>(
                        fetch: &mut <Self as WorldQueryGats<'w>>::Fetch,
                        table_row: usize,
                    ) -> <Self as WorldQueryGats<'w>>::Item {
                        <#deferred_worldquery as WorldQuery>::table_fetch(fetch, table_row)
                    }

                    fn update_component_access(state: &Self::State, access: &mut FilteredAccess<ComponentId>) {
                        <#deferred_worldquery as WorldQuery>::update_component_access(state, access)
                    }

                    fn update_archetype_component_access(
                        state: &Self::State,
                        archetype: &Archetype,
                        access: &mut Access<ArchetypeComponentId>,
                    ) {
                        <#deferred_worldquery as WorldQuery>::update_archetype_component_access(state, archetype, access)
                    }

                    fn init_state(world: &mut World) -> Self::State {
                        <#deferred_worldquery as WorldQuery>::init_state(world)
                    }

                    fn matches_component_set(
                        state: &Self::State,
                        set_contains_id: &impl Fn(ComponentId) -> bool,
                    ) -> bool {
                        <#deferred_worldquery as WorldQuery>::matches_component_set(state, set_contains_id)
                    }
                }
            };
        }
    };

    let world_query_impl_read_only = world_query_impler(true);
    let world_query_impl_mutable = world_query_impler(false);

    TokenStream::from(quote! {
        #trait_definition

        unsafe impl #impl_generics bevy::ecs::query::ReadOnlyWorldQuery for &'_ (dyn #trait_name #trait_generics + 'static) {}

        #world_query_impl_read_only
        #world_query_impl_mutable

        struct #meta_struct_name #impl_generics (
            for<'a> unsafe fn(bevy::ptr::Ptr<'a>) -> &'a (dyn #trait_name #trait_generics + 'static),
            for<'a> unsafe fn(bevy::ptr::PtrMut<'a>, &'a core::cell::UnsafeCell<bevy::ecs::component::ComponentTicks>, u32, u32) -> bevy_unofficial_trait_queries::Mut<'a, (dyn #trait_name #trait_generics + 'static)>,
        ) #where_clauses;

        impl #impl_generics bevy_unofficial_trait_queries::TraitQueryArg for (dyn #trait_name #trait_generics + 'static) #where_clauses {
            type Meta = #meta_struct_name #trait_generics;
            unsafe fn make_item<'a>(ptr: bevy::ptr::Ptr<'a>, meta: &'a Self::Meta) -> <Self as bevy_unofficial_trait_queries::TraitQueryArgGats<'a>>::Item {
                (meta.0)(ptr)
            }
            unsafe fn make_item_mut<'a>(ptr: bevy::ptr::PtrMut<'a>, ticks: &'a core::cell::UnsafeCell<bevy::ecs::component::ComponentTicks>, last_change_tick: u32, change_tick: u32, meta: &'a Self::Meta) -> <Self as bevy_unofficial_trait_queries::TraitQueryArgGats<'a>>::ItemMut {
                (meta.1)(ptr, ticks, last_change_tick, change_tick)
            }
        }

        unsafe impl #impl_generics_with_world bevy_unofficial_trait_queries::TraitQueryArgGats<'__w> for (dyn #trait_name #trait_generics + 'static) #where_clauses {
            type Item = &'__w (dyn #trait_name #trait_generics + 'static);
            type ItemMut = bevy_unofficial_trait_queries::Mut<'__w, (dyn #trait_name #trait_generics + 'static)>;
        }

        impl #impl_generics_with_t bevy_unofficial_trait_queries::SynthesiseMetaFrom<__T> for (dyn #trait_name #trait_generics + 'static) #where_clauses {
            fn make_meta() -> #meta_struct_name #trait_generics {
                #meta_struct_name(
                    |ptr| unsafe { ptr.deref::<__T>() },
                    |ptr, ticks, last_change_tick, change_tick| unsafe {
                        bevy_unofficial_trait_queries::Mut::__new_mut(&mut *ticks.get(), last_change_tick, change_tick, ptr.deref_mut::<__T>())
                    },
                )
            }
        }
    })
}
