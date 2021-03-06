// LNP/BP Rust Library
// Written in 2020 by
//     Rajarshi Maitra <rajarshi149@protonmail.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use std::collections::BTreeMap;

use amplify::Wrapper;
use lnpbp::client_side_validation::{
    CommitConceal, CommitEncode, ToMerkleSource,
};

use super::OwnedRightsInner;
use crate::schema::NodeType;
use crate::{Assignments, OwnedRights, OwnedState, StateTypes};

/// Merge Error generated in merging operation
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Display,
    From,
    Error,
)]
#[display(doc_comments)]
pub enum Error {
    /// Owned State has different commitment ids and can't be reveal-merged
    OwnedStateMismatch,

    /// Assignment has different commitment ids and can't be reveal-merged
    AssignmentMismatch,

    /// OwnedRights has different commitment ids and can't be reveal-merged
    OwnedRightsMismatch,

    /// Anchors has different commitment ids and can't be reveal-merged
    AnchorsMismatch,

    /// Node of type {0} has different commitment ids and can't be
    /// reveal-merged
    NodeMismatch(NodeType),
}

/// A trait to merge two structures modifying the revealed status
/// of the first one. The merge operation will **consume** both the structures
/// and return a new structure with revealed states.
///
/// The resulting structure will depend on the reveal status of both of the
/// variant. And the most revealed condition among the two will be selected
/// Usage: prevent hiding already known previous state data by merging
/// incoming new consignment in stash.
///
/// The follwoing conversion logic is intended by this trait:
///
/// merge (Revelaed, Anything) = Revealed
/// merge(ConfidentialSeal, ConfidentiualAmount) = Revealed
/// merge(ConfidentialAmount, ConfidentialSeal) = Revealed
/// merge(Confidential, Anything) = Anything
pub trait IntoRevealed: Sized {
    fn into_revealed(self, other: Self) -> Result<Self, Error>;
}

impl<STATE> IntoRevealed for OwnedState<STATE>
where
    Self: Clone,
    STATE: StateTypes,
    STATE::Confidential: PartialEq + Eq,
    STATE::Confidential:
        From<<STATE::Revealed as CommitConceal>::ConcealedCommitment>,
{
    fn into_revealed(self, other: Self) -> Result<Self, Error> {
        // if self and other is different through error
        if self.commit_serialize() != other.commit_serialize() {
            Err(Error::OwnedStateMismatch)
        } else {
            match (self, other) {
                // Anything + Revealed = Revealed
                (_, state @ OwnedState::Revealed { .. })
                | (state @ OwnedState::Revealed { .. }, _) => Ok(state),

                // ConfidentialAmount + ConfidentialSeal = Revealed
                (
                    OwnedState::ConfidentialSeal {
                        assigned_state: state,
                        ..
                    },
                    OwnedState::ConfidentialAmount {
                        seal_definition: seal,
                        ..
                    },
                ) => Ok(OwnedState::Revealed {
                    seal_definition: seal,
                    assigned_state: state,
                }),

                // ConfidentialSeal + ConfidentialAmount = Revealed
                (
                    OwnedState::ConfidentialAmount {
                        seal_definition: seal,
                        ..
                    },
                    OwnedState::ConfidentialSeal {
                        assigned_state: state,
                        ..
                    },
                ) => Ok(OwnedState::Revealed {
                    seal_definition: seal,
                    assigned_state: state,
                }),

                // if self and other is of same variant return self
                (
                    state @ OwnedState::ConfidentialAmount { .. },
                    OwnedState::ConfidentialAmount { .. },
                ) => Ok(state),
                (
                    state @ OwnedState::ConfidentialSeal { .. },
                    OwnedState::ConfidentialSeal { .. },
                ) => Ok(state),

                // Anything + Confidential = Anything
                (state, OwnedState::Confidential { .. })
                | (OwnedState::Confidential { .. }, state) => Ok(state),
            }
        }
    }
}

impl IntoRevealed for Assignments {
    fn into_revealed(self, other: Self) -> Result<Self, Error> {
        if self.consensus_commitments() != other.consensus_commitments() {
            Err(Error::AssignmentMismatch)
        } else {
            match (self, other) {
                (
                    Assignments::Declarative(first_vec),
                    Assignments::Declarative(second_vec),
                ) => {
                    let mut result = Vec::with_capacity(first_vec.len());
                    for (first, second) in
                        first_vec.into_iter().zip(second_vec.into_iter())
                    {
                        result.push(first.into_revealed(second)?);
                    }
                    Ok(Assignments::Declarative(result))
                }

                (
                    Assignments::DiscreteFiniteField(first_vec),
                    Assignments::DiscreteFiniteField(second_vec),
                ) => {
                    let mut result = Vec::with_capacity(first_vec.len());
                    for (first, second) in
                        first_vec.into_iter().zip(second_vec.into_iter())
                    {
                        result.push(first.into_revealed(second)?);
                    }
                    Ok(Assignments::DiscreteFiniteField(result))
                }

                (
                    Assignments::CustomData(first_vec),
                    Assignments::CustomData(second_vec),
                ) => {
                    let mut result = Vec::with_capacity(first_vec.len());
                    for (first, second) in
                        first_vec.into_iter().zip(second_vec.into_iter())
                    {
                        result.push(first.into_revealed(second)?);
                    }
                    Ok(Assignments::CustomData(result))
                }
                // No other patterns possible, should not reach here
                _ => {
                    unreachable!("Assignments::consensus_commitments is broken")
                }
            }
        }
    }
}

impl IntoRevealed for OwnedRightsInner {
    fn into_revealed(self, other: Self) -> Result<Self, Error> {
        if self.to_merkle_source().commit_serialize()
            != other.to_merkle_source().commit_serialize()
        {
            return Err(Error::OwnedRightsMismatch);
        }
        let mut result: OwnedRights = BTreeMap::new();
        for (first, second) in self
            .into_inner()
            .into_iter()
            .zip(other.into_inner().into_iter())
        {
            result.insert(first.0, first.1.into_revealed(second.1)?);
        }
        Ok(OwnedRightsInner::from_inner(result))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::strict_encoding::StrictDecode;
    use crate::ConcealState;
    use crate::{HashStrategy, PedersenStrategy};

    // Hard coded test vectors of Assignment Variants
    // Each Variant contains 4 types of Assignments
    // [Revealed, Confidential, ConfidentialSeal, ConfidentialState]
    static HASH_VARIANT: [u8; 267] = include!("../../test/hash_state.in");

    static PEDERSAN_VARIANT: [u8; 1664] =
        include!("../../test/pedersan_state.in");

    #[test]
    fn test_into_revealed_state() {
        let ass = Assignments::strict_decode(&PEDERSAN_VARIANT[..])
            .unwrap()
            .into_discrete_state();

        let rev = ass[1].clone();

        // Check Revealed + Anything = Revealed

        // Revealed + Revealed = Revealed
        let mut merged = rev.clone().into_revealed(rev.clone()).unwrap();
        assert_eq!(merged, rev);

        // Revealed + Confidential = Revealed
        let conf = rev.commit_conceal();
        merged = rev.clone().into_revealed(conf.clone()).unwrap();
        assert_eq!(merged, rev);

        // Revealed + Confidential State = Revealed
        let mut conf_state = rev.clone();
        conf_state.conceal_state();
        merged = rev.clone().into_revealed(conf_state.clone()).unwrap();
        assert_eq!(merged, rev);

        // Revealed + Confidential Seal = Revealed
        let seal = rev.seal_definition_confidential();
        let conf_seal = OwnedState::<PedersenStrategy>::ConfidentialSeal {
            seal_definition: seal,
            assigned_state: rev.assigned_state().unwrap().clone(),
        };
        merged = rev.clone().into_revealed(conf_seal.clone()).unwrap();
        assert_eq!(merged, rev);

        // Check Confidential Seal + Condfidential State = Revealed
        merged = conf_seal.clone().into_revealed(conf_state.clone()).unwrap();
        assert_eq!(merged, rev);

        // Check Condifential State + Confidential Seal = Revealed
        merged = conf_state.clone().into_revealed(conf_seal.clone()).unwrap();
        assert_eq!(merged, rev);

        // Check Confidential + Anything = Anything
        // Confidential + Reveal = Reveal
        merged = conf.clone().into_revealed(rev.clone()).unwrap();
        assert_eq!(merged, rev);

        // Confidential + Confidential Seal = Confidential Seal
        merged = conf.clone().into_revealed(conf_seal.clone()).unwrap();
        assert_eq!(merged, conf_seal);

        // Confidential + Confidential State = Confidential State
        merged = conf.clone().into_revealed(conf_state.clone()).unwrap();
        assert_eq!(merged, conf_state);

        // Confidential + Confidential = Confidential
        merged = conf.clone().into_revealed(conf.clone()).unwrap();
        assert_eq!(merged, conf);
    }

    #[test]
    fn test_into_revealed_assignements_ownedstates() {
        let assignment = Assignments::strict_decode(&HASH_VARIANT[..])
            .unwrap()
            .to_custom_state();

        // Get a revealed state
        let rev = assignment[3].clone();

        // Compute different exposure of the same state
        let conf = rev.clone().commit_conceal();

        let seal = rev.seal_definition_confidential();

        let conf_seal = OwnedState::<HashStrategy>::ConfidentialSeal {
            seal_definition: seal,
            assigned_state: rev.assigned_state().unwrap().clone(),
        };

        let mut conf_state = rev.clone();
        conf_state.conceal_state();

        // Create assignment for testing
        let test_variant_1 =
            vec![rev.clone(), conf_seal, conf_state, conf.clone()];
        let assignment_1 = Assignments::CustomData(test_variant_1.clone());

        // Create assignment 2 for testing
        // which is reverse of assignment 1
        let mut test_variant_2 = test_variant_1.clone();
        test_variant_2.reverse();
        let assignmnet_2 = Assignments::CustomData(test_variant_2);

        // Performing merge revelaing
        let merged = assignment_1
            .clone()
            .into_revealed(assignmnet_2.clone())
            .unwrap();

        // After merging all the states expeected be revealed
        for state in merged.to_custom_state() {
            assert_eq!(state, rev);
        }

        // Test against confidential merging
        // Confidential + Anything = Anything
        let test_variant_3 =
            vec![conf.clone(), conf.clone(), conf.clone(), conf.clone()];
        let assignment_3 = Assignments::CustomData(test_variant_3);

        // merge with assignment 1
        let merged = assignment_3
            .clone()
            .into_revealed(assignment_1.clone())
            .unwrap();

        assert_eq!(assignment_1, merged);

        // test for OwnedRights structure
        let test_owned_rights_1: OwnedRightsInner =
            bmap! { 1usize => assignment_1.clone()}.into();
        let test_owned_rights_2: OwnedRightsInner =
            bmap! { 1usize => assignmnet_2.clone()}.into();

        // Perform merge
        let merged = test_owned_rights_1
            .clone()
            .into_revealed(test_owned_rights_2.clone())
            .unwrap();

        // after merge operation all the states will be revealed
        let states = vec![rev.clone(), rev.clone(), rev.clone(), rev.clone()];
        let assgn = Assignments::CustomData(states);
        let expected_rights: OwnedRightsInner = bmap! {1usize => assgn}.into();

        assert_eq!(merged, expected_rights);
    }
}
