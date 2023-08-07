mod broadcast;
pub(crate) mod error;
pub(crate) mod signed_message;
mod states;
mod type_erased;

use rand_core::CryptoRngCore;
use serde::{Deserialize, Serialize};
use signature::hazmat::{PrehashVerifier, RandomizedPrehashSigner};

use crate::curve::{RecoverableSignature, Scalar};
use crate::protocols::{
    auxiliary,
    common::{KeyShare, KeyShareChange, KeyShareSeed, PartyIdx},
    interactive_signing, keygen,
};
use crate::SchemeParams;

pub use crate::protocols::generic::InitError;
pub use error::Error;
pub use signed_message::SignedMessage;
pub use states::{FinalizeOutcome, SendingState, ToSend};

pub type PrehashedMessage = [u8; 32];

pub fn make_keygen_session<P, Sig, Signer, Verifier>(
    rng: &mut impl CryptoRngCore,
    shared_randomness: &[u8],
    signer: Signer,
    verifiers: &[Verifier],
    party_idx: PartyIdx,
) -> Result<SendingState<KeyShareSeed, Sig, Signer, Verifier>, InitError>
where
    Sig: Clone + Serialize + for<'de> Deserialize<'de> + PartialEq + Eq,
    P: SchemeParams + 'static,
    Signer: RandomizedPrehashSigner<Sig>,
    Verifier: PrehashVerifier<Sig> + Clone,
{
    SendingState::new::<keygen::Round1<P>>(rng, shared_randomness, signer, party_idx, verifiers, ())
}

pub fn make_key_refresh_session<P, Sig, Signer, Verifier>(
    rng: &mut impl CryptoRngCore,
    shared_randomness: &[u8],
    signer: Signer,
    verifiers: &[Verifier],
    party_idx: PartyIdx,
) -> Result<SendingState<KeyShareChange<P>, Sig, Signer, Verifier>, InitError>
where
    Sig: Clone + Serialize + for<'de> Deserialize<'de> + PartialEq + Eq,
    P: SchemeParams + 'static,
    Signer: RandomizedPrehashSigner<Sig>,
    Verifier: PrehashVerifier<Sig> + Clone,
{
    SendingState::new::<auxiliary::Round1<P>>(
        rng,
        shared_randomness,
        signer,
        party_idx,
        verifiers,
        (),
    )
}

pub fn make_interactive_signing_session<P, Sig, Signer, Verifier>(
    rng: &mut impl CryptoRngCore,
    shared_randomness: &[u8],
    signer: Signer,
    verifiers: &[Verifier],
    key_share: &KeyShare<P>,
    prehashed_message: &PrehashedMessage,
) -> Result<SendingState<RecoverableSignature, Sig, Signer, Verifier>, InitError>
where
    Sig: Clone + Serialize + for<'de> Deserialize<'de> + PartialEq + Eq,
    P: SchemeParams + 'static,
    Signer: RandomizedPrehashSigner<Sig>,
    Verifier: PrehashVerifier<Sig> + Clone,
{
    let scalar_message = Scalar::from_reduced_bytes(prehashed_message);

    let context = interactive_signing::Context {
        key_share: key_share.clone(),
        message: scalar_message,
    };

    SendingState::new::<interactive_signing::Round1Part1<P>>(
        rng,
        shared_randomness,
        signer,
        key_share.party_index(),
        verifiers,
        context,
    )
}
