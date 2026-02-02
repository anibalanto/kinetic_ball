use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct Team {
    pub starters: HashSet<u32>,
    pub substitutes: HashSet<u32>,
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct MatchSlots {
    pub teams: [Team; 2],
    pub spectators: HashSet<u32>,
    pub admins: HashSet<u32>,
}

impl MatchSlots {
    /// Finds which slot a player is in
    /// Returns: (team_index: Option<u8>, is_starter: Option<bool>)
    /// If spectator: (None, None)
    pub fn find_player(&self, player_id: u32) -> (Option<u8>, Option<bool>) {
        for (team_idx, team) in self.teams.iter().enumerate() {
            if team.starters.contains(&player_id) {
                return (Some(team_idx as u8), Some(true));
            }
            if team.substitutes.contains(&player_id) {
                return (Some(team_idx as u8), Some(false));
            }
        }
        if self.spectators.contains(&player_id) {
            return (None, None);
        }
        (None, None)
    }

    /// Removes a player from any slot
    pub fn remove_player(&mut self, player_id: u32) {
        for team in &mut self.teams {
            team.starters.remove(&player_id);
            team.substitutes.remove(&player_id);
        }
        self.spectators.remove(&player_id);
    }

    /// Moves a player to a new position
    pub fn move_player(
        &mut self,
        player_id: u32,
        team_index: Option<u8>,
        is_starter: Option<bool>,
    ) {
        self.remove_player(player_id);
        match (team_index, is_starter) {
            (Some(t), Some(true)) => {
                self.teams[t as usize].starters.insert(player_id);
            }
            (Some(t), Some(false)) => {
                self.teams[t as usize].substitutes.insert(player_id);
            }
            _ => {
                self.spectators.insert(player_id);
            }
        }
    }

    /// Adds a player as spectator (default on join)
    pub fn add_spectator(&mut self, player_id: u32) {
        self.spectators.insert(player_id);
    }

    /// Adds a player as starter on a team (alternating teams)
    pub fn add_starter(&mut self, player_id: u32, team_index: u8) {
        self.teams[team_index as usize].starters.insert(player_id);
    }

    /// Removes admin status from a player
    pub fn remove_admin(&mut self, player_id: u32) {
        self.admins.remove(&player_id);
    }

    /// Checks if a player is an admin
    pub fn is_admin(&self, player_id: u32) -> bool {
        self.admins.contains(&player_id)
    }

    /// Adds an admin
    pub fn add_admin(&mut self, player_id: u32) {
        self.admins.insert(player_id);
    }

    /// Checks if a player is a starter (on field)
    pub fn is_starter(&self, player_id: u32) -> bool {
        self.teams
            .iter()
            .any(|team| team.starters.contains(&player_id))
    }

    /// Checks if a player is a substitute
    pub fn is_substitute(&self, player_id: u32) -> bool {
        self.teams
            .iter()
            .any(|team| team.substitutes.contains(&player_id))
    }

    /// Checks if a player is a spectator
    pub fn is_spectator(&self, player_id: u32) -> bool {
        self.spectators.contains(&player_id)
    }

    /// Gets all player IDs that are starters (on field)
    pub fn get_all_starters(&self) -> Vec<u32> {
        let mut starters = Vec::new();
        for team in &self.teams {
            starters.extend(team.starters.iter().copied());
        }
        starters
    }

    /// Gets the team index for a player, if they're on a team
    pub fn get_team_index(&self, player_id: u32) -> Option<u8> {
        self.find_player(player_id).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_spectator() {
        let mut slots = MatchSlots::default();
        slots.add_spectator(1);
        assert!(slots.is_spectator(1));
        assert!(!slots.is_starter(1));
    }

    #[test]
    fn test_move_player_to_starter() {
        let mut slots = MatchSlots::default();
        slots.add_spectator(1);
        slots.move_player(1, Some(0), Some(true));
        assert!(slots.is_starter(1));
        assert!(!slots.is_spectator(1));
        assert_eq!(slots.get_team_index(1), Some(0));
    }

    #[test]
    fn test_move_player_to_substitute() {
        let mut slots = MatchSlots::default();
        slots.add_spectator(1);
        slots.move_player(1, Some(1), Some(false));
        assert!(slots.is_substitute(1));
        assert!(!slots.is_spectator(1));
        assert_eq!(slots.get_team_index(1), Some(1));
    }

    #[test]
    fn test_move_player_back_to_spectator() {
        let mut slots = MatchSlots::default();
        slots.move_player(1, Some(0), Some(true));
        slots.move_player(1, None, None);
        assert!(slots.is_spectator(1));
        assert!(!slots.is_starter(1));
    }
}
