use crate::engine::{
    add_actor_spell_element, complete_directed_magick, self_magick, start_area_of_effect_magick,
    start_directed_magick,
};
use crate::protocol::ActorAction;
use crate::world::World;

pub fn apply_actor_action(actor_action: &ActorAction, actor_index: usize, world: &mut World) {
    match actor_action {
        ActorAction::Move(moving) => {
            world.actors[actor_index].moving = *moving;
        }
        ActorAction::SetTargetDirection(target_direction) => {
            world.actors[actor_index].target_direction = *target_direction;
        }
        ActorAction::AddSpellElement(element) => {
            add_actor_spell_element(actor_index, *element, world);
        }
        ActorAction::StartDirectedMagick => {
            start_directed_magick(actor_index, world);
        }
        ActorAction::CompleteDirectedMagick => {
            complete_directed_magick(actor_index, world);
        }
        ActorAction::SelfMagick => {
            self_magick(actor_index, world);
        }
        ActorAction::StartAreaOfEffectMagick => {
            start_area_of_effect_magick(actor_index, world);
        }
    }
}
