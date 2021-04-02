use crate::engine::{
    add_actor_spell_element, complete_directed_magick, self_magick, start_area_of_effect_magick,
    start_directed_magick,
};
use crate::protocol::PlayerAction;
use crate::world::World;

pub fn apply_player_action(player_action: &PlayerAction, actor_index: usize, world: &mut World) {
    match player_action {
        PlayerAction::Move(moving) => {
            world.actors[actor_index].moving = *moving;
        }
        PlayerAction::SetTargetDirection(target_direction) => {
            world.actors[actor_index].target_direction = *target_direction;
        }
        PlayerAction::AddSpellElement(element) => {
            add_actor_spell_element(actor_index, *element, world);
        }
        PlayerAction::StartDirectedMagick => {
            start_directed_magick(actor_index, world);
        }
        PlayerAction::CompleteDirectedMagick => {
            complete_directed_magick(actor_index, world);
        }
        PlayerAction::SelfMagick => {
            self_magick(actor_index, world);
        }
        PlayerAction::StartAreaOfEffectMagick => {
            start_area_of_effect_magick(actor_index, world);
        }
    }
}
