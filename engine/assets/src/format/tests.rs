//! Tests for format encoding, validation, and transcoding.
use std::error::Error;
use std::io;

use super::*;

fn pack() -> MeshPack {
    MeshPack {
        source_hash: 71,
        meshes: Vec::new(),
        materials: Vec::new(),
        nodes: vec![Node {
            name: "TAG_Socket_Muzzle".into(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: Vec::new(),
        }],
        clips: Vec::new(),
        collision: vec![CollisionSet {
            node: 0,
            surface: "concrete".into(),
            is_sensor: false,
            shapes: vec![CollisionShape::TriMesh {
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                indices: vec![0, 2, 1],
            }],
        }],
        textures: Vec::new(),
    }
}

#[test]
fn collision_round_trips_and_bad_indices_are_rejected() -> Result<(), Box<dyn Error>> {
    let asset = Asset::MeshPack(pack());
    let bytes = encode(&asset, 71)?;
    assert_eq!(decode(&bytes)?.1, asset);

    let mut invalid = pack();
    if let Some(set) = invalid.collision.first_mut()
        && let Some(CollisionShape::TriMesh { indices, .. }) = set.shapes.first_mut()
        && let Some(first) = indices.first_mut()
    {
        *first = 99;
    }
    let bytes = encode(&Asset::MeshPack(invalid), 71)?;
    assert!(matches!(decode(&bytes), Err(AssetError::Structure)));
    Ok(())
}

#[test]
fn texture_transcodes_to_rgba8_and_bc7_with_expected_base_sizes() -> Result<(), Box<dyn Error>> {
    let texture = TextureData {
        ktx2: include_bytes!("../../tests/fixtures/uastc.ktx2").to_vec(),
    };
    let rgba = texture.transcode(GpuTextureFormatFamily::Rgba8)?;
    let bc7 = texture.transcode(GpuTextureFormatFamily::Bc7)?;
    assert!(
        rgba.len() > 1,
        "UASTC texture must include its generated mip chain"
    );
    let rgba_base = rgba
        .first()
        .ok_or_else(|| io::Error::other("RGBA base mip missing"))?;
    let bc7_base = bc7
        .first()
        .ok_or_else(|| io::Error::other("BC7 base mip missing"))?;
    assert_eq!(
        rgba_base.bytes.len(),
        usize::try_from(rgba_base.width * rgba_base.height * 4)?
    );
    assert_eq!(
        bc7_base.bytes.len(),
        usize::try_from(bc7_base.width.div_ceil(4) * bc7_base.height.div_ceil(4) * 16)?
    );
    Ok(())
}

#[test]
fn every_gpu_family_transcodes_the_full_mip_chain_with_valid_block_sizes()
-> Result<(), Box<dyn Error>> {
    let texture = TextureData {
        ktx2: include_bytes!("../../tests/fixtures/uastc.ktx2").to_vec(),
    };
    let families = [
        (GpuTextureFormatFamily::Bc7, 16usize),
        (GpuTextureFormatFamily::Astc4x4, 16),
        (GpuTextureFormatFamily::Etc2, 16),
        (GpuTextureFormatFamily::Rgba8, 4),
    ];
    let reference = texture.transcode(GpuTextureFormatFamily::Rgba8)?;
    for (family, bytes_per_block) in families {
        let mips = texture.transcode(family)?;
        assert_eq!(mips.len(), reference.len());
        for (mip, reference_mip) in mips.iter().zip(reference.iter()) {
            assert_eq!(
                (mip.width, mip.height),
                (reference_mip.width, reference_mip.height)
            );
            let expected_bytes = if family == GpuTextureFormatFamily::Rgba8 {
                usize::try_from(mip.width * mip.height * 4)?
            } else {
                usize::try_from(mip.width.div_ceil(4) * mip.height.div_ceil(4))?
                    .saturating_mul(bytes_per_block)
            };
            assert_eq!(mip.bytes.len(), expected_bytes);
        }
    }
    Ok(())
}

#[test]
fn invalid_texture_returns_a_typed_transcode_reason() {
    let texture = TextureData {
        ktx2: b"not KTX2".to_vec(),
    };
    assert!(matches!(
        texture.transcode(GpuTextureFormatFamily::Rgba8),
        Err(AssetError::Transcode(TextureTranscodeError::InvalidKtx2))
    ));
}

#[test]
fn versioned_pack_round_trips_with_hash_and_socket_view() -> Result<(), Box<dyn Error>> {
    let asset = Asset::MeshPack(pack());
    let bytes = encode(&asset, 71)?;
    let (hash, decoded) = decode(&bytes)?;
    assert_eq!(hash, 71);
    assert_eq!(decoded, asset);
    let Asset::MeshPack(decoded) = decoded;
    assert_eq!(decoded.sockets().collect::<Vec<_>>(), vec![("Muzzle", 0)]);
    let transform = decoded
        .node_world_transform(0)
        .ok_or_else(|| io::Error::other("socket transform missing"))?;
    assert_eq!(
        transform.transform_point3(glam::Vec3::ZERO),
        glam::Vec3::ZERO
    );
    Ok(())
}

proptest::proptest! {
    #[test]
    fn random_corruption_never_panics_and_returns_a_typed_error(offset in 0usize..64, bit in 0u8..8) {
        let Ok(original) = encode(&Asset::MeshPack(pack()), 71) else {
            proptest::prop_assert!(false, "fixture encoding failed");
            return Ok(());
        };
        let mut corrupted = original;
        let selected = offset % corrupted.len();
        if let Some(byte) = corrupted.get_mut(selected) { *byte ^= 1_u8 << bit; }
        proptest::prop_assert!(decode(&corrupted).is_err());
    }
}
