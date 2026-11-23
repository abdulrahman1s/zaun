//! Shared fixtures of the agent tests: links the live CLI (7.1.4) printed, and the values of simplexmq's ConnectionRequestTests.
use zaun_chat::agent::links::ConnectionRequestUri;
use zaun_chat::agent::version::supported_client_range;
use zaun_chat::agent::{E2eParamsUri, QueueMode, SmpQueueAddress, SmpQueueUri, SmpServer};
use zaun_chat::crypto::PublicKey;
use zaun_chat::encoding::{Version, VersionRange, b64_decode};

pub const CLI_INVITATION_FULL: &str = "simplex:/invitation#/?v=6-8&smp=smp%3A%2F%2Fhpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg%3D%40smp5.simplex.im%2Fyi9ZNNpE3mzyHwrpvPPyWwQpoK42ILoz%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEApJwJDPPVaHvZPkczx1rzjoJ3nKatAU99E2b0bcYwy3k%253D%26q%3Dm%26k%3Ds%26srv%3Djjbyvoemxysm7qxap7m5d5m35jzv5qq6gnlv7s4rsn7tdwwmuqciwpid.onion&e2e=v%3D3%26x3dh%3DMEIwBQYDK2VvAzkAVkxeHJWln7lvt-jG2oqQcnZEUwp_RtHgg0IqooPlEH5Kgu0nSkNYMSEZJX5QgPoCXqmaooRyq-k%3D%2CMEIwBQYDK2VvAzkAokeXtU1DtQIE9QtNOWKoLVbbNNvbYz_zAk5TdvQb0fZ0yq0wEWa4WrotO7hysu52F7E5Iu-Z31g%3D%26kem_key%3DLsS6o6NuksC43VLWtoOM4etRjrR4QOHk_feSKqx_R4ZGChcFbSr-PkQfNSi9IM4yuMvzzlZD3pHY0887PfLoQKY5415_qKQwlCMctbMlqWic8SayLePnOxEWogF7rLB1XMfbYGOivgA82UHRybP45-TPFxKMuolv8hwmklyNRUEYP9QCP3xmcEdIF0OVsgru83xxdDI0wEWJvaOQFeZiTlaR6ct4Sh3NVqVmCtKSG-gXfVBWm2lOFPIPsa-xJv8evRJWHmnsFr_JmwP_-EBUyVNr4eYeHOkjxfX3tttfDlYKoqYu26hp-OuZZFXhvs2F7Rp7LcNyvFuJ7NEaOTmi00wO6pZb3MRo3Z0f8ab90kn6Whd0EA4gkfLuPDJptkHt96tkx--wTyJeX2wMwB6lLBNewdbIwvGrVGv15pDiVu2NwnFluKCxDjm69M4NRP2K55n-gaYfRzmr0EZvjnsUD_2SG8-KPCquLRaxcaTkQFtOsT18eVcnMxhwpSu5LuxNt7I2Z_icA6jV-Oh0y4g8vXbbNLwb5rUGW3rqWFpUqIi4atTXLflXWUSFAk48v6W8P9awIXEswBaWQoIp9YlXJUItVLFF_CcGw0p-ZYmaedQyda4p3p1qwhOPvTkofhGaD9eu8Hczlf4Wv5ptBgnESo96iTz2tS_VnXueUihUhpwUWkLGy36VisdoIs9YLMr3KjLg3pfy1OzJCa2gvRrnldBdabBpde8OHl7-tEk_ixEkqwGWf2IH3Mzhjwm4HU4XsAid_wDjM8VCynQ6pg6EKEzw800rCyw87_jfmeTBxbLfFIatB5_139oQiGuv_p7rWKjjDjT-tPqyamPo6xYcgVDUUKsX7FuRYt01-v1mS1SBIbDG7yugkl9qWfTmZ3BvY6JW1t9hdUmCUo6x-RdZmur5UljUPEK83wiEGHHTgXi-rro-_sr2SzYxfaDogW-kat9HNxhYoNz1QgkmazzkBbJlASJuHt3z6xyV2cRzFlVEJgsdKO8MWbibO-xbsfhkxudvd9gYB5CRnwRVgIwW1V1Pl21X0FRPqVIasPrUkS296L9PhpDaFsVxs_Zxm00xoYVG3pAGD_-pJhpG-tBFgGFhx7nRg0FDyybcgoWaVf7klmN6nRymqnB1uV3CMyI_VwB8UpsT2DfunKqAOsxVVp-aaCGQSp5ow12huPwSiMpQvMfS_L736w3wwNVtoY9iEnPTks59woTMdWG4-s-pvJFr_Y-Ed3gfdCQN2Np7Soc2gQWZx8XHb9W0DuD1f8CJDwuvVq4pALRlBPN3ZMa_65rMxqeFJOSXU2k6sx7dxAmfMy808Kg4nlj4ivthQFHsE9Q5DPCO2ShPZaAM702Y8Ud70Td17rVOURmWj8VDQARTY4EEwWTMDCgTEd2f-T7uriTPCGpgSM5RNAmapigz5pHCBsbblgmhYb57SCN_2YtVV1gu4mJYxvg5bSlTnG65Oke3yTECNCRMttzsooTHNyqyZwORmslmHYAfBKb12t4-_ZmLUdAWke5jId4sZIDSfLbU-AoF";
pub const CLI_INVITATION_SHORT: &str = "https://smp5.simplex.im/i#esW_-BXJI9zs4iGRV2SMHUzqpFv3QRe6/KaKvW0lOkJzcvIRVW-l0uomDrjYKjiaOkPIkxSvOB0o";
pub const CLI_CONTACT_FULL: &str = "simplex:/contact#/?v=6-8&smp=smp%3A%2F%2FPQUV2eL0t7OStZOoAsPEV2QYWt4-xilbakvGUGOItUo%3D%40smp6.simplex.im%2FUKN0gf7WnXXNs1k0FUleanVhaQysygYI%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEApIv2f5DPQiNCNcOUciZAOS3jl45pQxZiG4RwENuJlDE%253D%26q%3Dc%26srv%3Dbylepyau3ty4czmn77q4fglvperknl4bi2eb2fdy2bh4jxtf32kf73yd.onion";
pub const CLI_CONTACT_SHORT: &str =
    "https://smp6.simplex.im/a#0sPWloEa5zFH0sfc_vANe4RUUebBQSGcvTKpKRH300s";

pub fn key(b64: &str) -> PublicKey {
    PublicKey::from_der(&b64_decode(b64).unwrap()).unwrap()
}

pub fn range(min: u16, max: u16) -> VersionRange {
    VersionRange::new(Version(min), Version(max)).unwrap()
}

/// `srv` of ConnectionRequestTests.
pub fn srv() -> SmpServer {
    SmpServer {
        hosts: vec![
            "smp.simplex.im".into(),
            "jjbyvoemxysm7qxap7m5d5m35jzv5qq6gnlv7s4rsn7tdwwmuqciwpid.onion".into(),
        ],
        port: Some(5223),
        key_hash: vec![215, 109, 248, 251],
        basic_auth: None,
    }
}

pub fn srv1() -> SmpServer {
    SmpServer {
        hosts: vec!["smp.simplex.im".into()],
        ..srv()
    }
}

pub fn test_dh_key() -> PublicKey {
    key("MCowBQYDK2VuAyEAjiswwI3O/NlS8Fk3HJUW870EY2bAwmttMBsvRB9eV3o=")
}

pub fn queue_addr(server: SmpServer, mode: Option<QueueMode>) -> SmpQueueAddress {
    SmpQueueAddress {
        server,
        sender_id: vec![223, 142, 122, 251],
        dh_public_key: test_dh_key(),
        queue_mode: mode,
    }
}

pub fn queue(range: VersionRange, address: SmpQueueAddress) -> SmpQueueUri {
    SmpQueueUri {
        client_v_range: range,
        address,
    }
}

/// Supported range 1-4, so the legacy encoding.
pub fn queue_supported() -> SmpQueueUri {
    queue(
        supported_client_range(),
        queue_addr(srv(), Some(QueueMode::Messaging)),
    )
}

pub fn test_e2e() -> E2eParamsUri {
    let k = key(
        "MEIwBQYDK2VvAzkAmKuSYeQ/m0SixPDS8Wq8VBaTS1cW+Lp0n0h4Diu+kUpR+qXx4SDJ32YGEFoGFGSbGPry5Ychr6U=",
    );
    E2eParamsUri {
        v_range: range(3, 3),
        key1: k.clone(),
        key2: k,
        kem: None,
    }
}

pub fn inv(link: &str) -> ConnectionRequestUri {
    ConnectionRequestUri::parse(link).unwrap()
}
