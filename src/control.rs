use crate::engine::{
    add_actor_spell_element, complete_directed_magick, self_magick, start_area_of_effect_magick,
    start_directed_magick,
};
use crate::protocol::{ActorAction, CastAction};
use crate::world::World;

pub fn apply_actor_action(actor_action: ActorAction, actor_index: usize, world: &mut World) {
    world.actors[actor_index].moving = actor_action.moving;
    world.actors[actor_index].target_direction = actor_action.target_direction;
    if let Some(cast_action) = actor_action.cast_action {
        apply_cast_action(cast_action, actor_index, world);
    }
}

pub fn apply_cast_action(cast_action: CastAction, actor_index: usize, world: &mut World) {
    match cast_action {
        CastAction::AddSpellElement(element) => {
            add_actor_spell_element(actor_index, element, world);
        }
        CastAction::StartDirectedMagick => {
            start_directed_magick(actor_index, world);
        }
        CastAction::CompleteDirectedMagick => {
            complete_directed_magick(actor_index, world);
        }
        CastAction::SelfMagick => {
            self_magick(actor_index, world);
        }
        CastAction::StartAreaOfEffectMagick => {
            start_area_of_effect_magick(actor_index, world);
        }
    }
}
