use alloc::boxed::Box;

use k256::ecdsa::VerifyingKey;
use rand_core::CryptoRngCore;
use serde::{Deserialize, Serialize};

use crate::cggmp21::SchemeParams;
use crate::curve::{Point, Scalar};
use crate::paillier::{PaillierParams, PublicKeyPaillier, SecretKeyPaillier};
use crate::tools::hashing::{Chain, Hashable};
use crate::uint::Zero;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PartyIdx(u32);

impl PartyIdx {
    pub fn as_usize(self) -> usize {
        self.0.try_into().unwrap()
    }

    pub fn from_usize(val: usize) -> Self {
        Self(val.try_into().unwrap())
    }
}

impl Hashable for PartyIdx {
    fn chain<C: Chain>(&self, digest: C) -> C {
        digest.chain(&self.0)
    }
}

/// The result of the Keygen protocol.
#[derive(Clone)]
pub struct KeyShareSeed {
    /// Secret key share of this node.
    pub secret_share: Scalar, // `x_i`
    /// Public key shares of all nodes (including this one).
    pub public_shares: Box<[Point]>, // `X_j`
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "SecretAuxInfo<P>: Serialize,
        PublicAuxInfo<P>: Serialize"))]
#[serde(bound(deserialize = "SecretAuxInfo<P>: for<'x> Deserialize<'x>,
        PublicAuxInfo<P>: for <'x> Deserialize<'x>"))]
pub struct KeyShare<P: SchemeParams> {
    pub(crate) index: PartyIdx,
    pub(crate) secret_share: Scalar,
    pub(crate) public_shares: Box<[Point]>,
    pub(crate) secret_aux: SecretAuxInfo<P>,
    pub(crate) public_aux: Box<[PublicAuxInfo<P>]>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "SecretKeyPaillier<P::Paillier>: Serialize"))]
#[serde(bound(deserialize = "SecretKeyPaillier<P::Paillier>: for <'x> Deserialize<'x>"))]
pub(crate) struct SecretAuxInfo<P: SchemeParams> {
    pub(crate) paillier_sk: SecretKeyPaillier<P::Paillier>,
    pub(crate) el_gamal_sk: Scalar, // `y_i`
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "PublicKeyPaillier<P::Paillier>: Serialize"))]
#[serde(bound(deserialize = "PublicKeyPaillier<P::Paillier>: for <'x> Deserialize<'x>"))]
pub(crate) struct PublicAuxInfo<P: SchemeParams> {
    pub(crate) el_gamal_pk: Point, // `Y_i`
    /// The Paillier public key.
    pub(crate) paillier_pk: PublicKeyPaillier<P::Paillier>,
    /// The ring-Pedersen generator.
    pub(crate) rp_generator: <P::Paillier as PaillierParams>::DoubleUint, // `t_i`
    /// The ring-Pedersen power (a number belonging to the group produced by the generator).
    pub(crate) rp_power: <P::Paillier as PaillierParams>::DoubleUint, // `s_i`
}

/// The result of the Auxiliary Info & Key Refresh protocol - the update to the key share.
#[derive(Clone)]
pub struct KeyShareChange<P: SchemeParams> {
    pub(crate) index: PartyIdx,
    /// The value to be added to the secret share.
    pub(crate) secret_share_change: Scalar, // `x_i^* - x_i == \sum_{j} x_j^i`
    /// The values to be added to the public shares of remote nodes.
    pub(crate) public_share_changes: Box<[Point]>, // `X_k^* - X_k == \sum_j X_j^k`, for all nodes
    pub(crate) secret_aux: SecretAuxInfo<P>,
    pub(crate) public_aux: Box<[PublicAuxInfo<P>]>,
}

/// The result of the Presigning protocol.
#[derive(Clone)]
pub struct PresigningData {
    pub(crate) nonce: Point, // `R`
    /// An additive share of the ephemeral scalar `k`.
    pub(crate) ephemeral_scalar_share: Scalar, // `k_i`
    /// An additive share of `k * x` where `x` is the secret key.
    pub(crate) product_share: Scalar,
}

impl<P: SchemeParams> KeyShare<P> {
    pub fn new(seed: KeyShareSeed, change: KeyShareChange<P>) -> Self {
        // TODO: check that party_idx is the same for both, and the number of parties is the same
        let secret_share = seed.secret_share + change.secret_share_change;
        let public_shares = seed
            .public_shares
            .iter()
            .zip(change.public_share_changes.into_vec())
            .map(|(public_share, public_share_change)| public_share + &public_share_change)
            .collect();
        Self {
            index: change.index,
            secret_share,
            public_shares,
            secret_aux: change.secret_aux,
            public_aux: change.public_aux,
        }
    }

    /// Returns `num_parties` of random self-consistent key shares
    /// (which in a decentralized case would be the output of KeyGen + Auxiliary protocols).
    pub fn new_centralized(
        rng: &mut impl CryptoRngCore,
        num_parties: usize,
        signing_key: Option<&k256::ecdsa::SigningKey>,
    ) -> Box<[Self]> {
        let secret = match signing_key {
            None => Scalar::random(rng),
            Some(sk) => Scalar::from(sk.as_nonzero_scalar()),
        };

        let secret_shares = secret.split(rng, num_parties);
        let public_shares = secret_shares
            .iter()
            .map(|s| s.mul_by_generator())
            .collect::<Box<_>>();

        let (secret_aux, public_aux) = make_aux_info(rng, num_parties);

        secret_aux
            .into_vec()
            .into_iter()
            .enumerate()
            .map(|(idx, secret_aux)| KeyShare {
                index: PartyIdx::from_usize(idx),
                secret_share: secret_shares[idx],
                public_shares: public_shares.clone(),
                secret_aux,
                public_aux: public_aux.clone(),
            })
            .collect()
    }

    pub fn update(self, change: KeyShareChange<P>) -> Self {
        // TODO: check that party_idx is the same for both, and the number of parties is the same
        let secret_share = self.secret_share + change.secret_share_change;
        let public_shares = self
            .public_shares
            .iter()
            .zip(change.public_share_changes.into_vec())
            .map(|(public_share, public_share_change)| public_share + &public_share_change)
            .collect();
        Self {
            index: change.index,
            secret_share,
            public_shares,
            secret_aux: change.secret_aux,
            public_aux: change.public_aux,
        }
    }

    pub(crate) fn verifying_key_as_point(&self) -> Point {
        self.public_shares.iter().sum()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        // TODO: need to ensure on creation of the share that the verifying key actually exists
        // (that is, the sum of public keys does not evaluate to the infinity point)
        self.verifying_key_as_point().to_verifying_key().unwrap()
    }

    pub fn num_parties(&self) -> usize {
        // TODO: technically it is `num_shares`, but for now we are equating the two,
        // since we assume that one party has one share.
        self.public_shares.len()
    }

    pub fn party_index(&self) -> PartyIdx {
        // TODO: technically it is the share index, but for now we are equating the two,
        // since we assume that one party has one share.
        self.index
    }
}

impl<P: SchemeParams> core::fmt::Debug for KeyShare<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(
            f,
            "KeyShare(idx={}, vkey={})",
            self.index.as_usize(),
            hex::encode(self.verifying_key_as_point().to_compressed_array())
        )
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn make_aux_info<P: SchemeParams>(
    rng: &mut impl CryptoRngCore,
    num_parties: usize,
) -> (Box<[SecretAuxInfo<P>]>, Box<[PublicAuxInfo<P>]>) {
    let secret_aux = (0..num_parties)
        .map(|_| SecretAuxInfo {
            paillier_sk: SecretKeyPaillier::<P::Paillier>::random(rng),
            el_gamal_sk: Scalar::random(rng),
        })
        .collect::<Box<_>>();

    let public_aux = secret_aux
        .iter()
        .map(|secret| PublicAuxInfo {
            paillier_pk: secret.paillier_sk.public_key(),
            el_gamal_pk: secret.el_gamal_sk.mul_by_generator(),
            rp_generator: <P::Paillier as PaillierParams>::DoubleUint::ZERO, // TODO: currently unused in the protocol
            rp_power: <P::Paillier as PaillierParams>::DoubleUint::ZERO, // TODO: currently unused in the protocol
        })
        .collect();

    (secret_aux, public_aux)
}

#[cfg(test)]
mod tests {
    use k256::ecdsa::SigningKey;
    use rand_core::OsRng;

    use super::KeyShare;
    use crate::TestSchemeParams;

    #[test]
    fn key_share_centralized() {
        let sk = SigningKey::random(&mut OsRng);
        let shares = KeyShare::<TestSchemeParams>::new_centralized(&mut OsRng, 3, Some(&sk));
        assert_eq!(&shares[0].verifying_key(), sk.verifying_key());
    }
}