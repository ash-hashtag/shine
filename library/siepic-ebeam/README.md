# SiEPIC EBeam compact models

S-parameter compact models from the [SiEPIC EBeam PDK](https://github.com/SiEPIC/SiEPIC_EBeam_PDK),
the open process kit of the UBC/SiEPIC electron-beam silicon photonics process, for its
nominal process: 220 nm silicon, 500 nm strip waveguides, TE polarisation, C band. They
are FDTD simulations (Lumerical INTERCONNECT format). Shine shows them as the
**SiEPIC EBeam** folder of the asset library.

They are copied unchanged, renamed only, from commit
[`363fbb52eefa`](https://github.com/SiEPIC/SiEPIC_EBeam_PDK/tree/363fbb52eefa8669100499ab531d3fe7befc4867) and are under the
PDK's MIT licence ([LICENSE.md](LICENSE.md), © 2016–2020 Lukas Chrostowski and
contributors). The PDK has no separate licence for its compact-model data.

| File here | In the PDK |
|---|---|
| Broadband 50-50 coupler.sparam | `klayout/EBeam/CML/EBeam/source_data/bdc_TE_source/bdc_Thickness =220 width=500.sparam` |
| Y-branch.sparam | `Lumerical_EBeam_CML/EBeam/source_data/y_branch_source/Ybranch_Thickness =220 width=500.sparam` |
| Directional coupler, gap 200 nm, length *L* µm.sparam (*L* = 0, 5, … 30) | `klayout/EBeam/CML/EBeam/source_data/ebeam_dc_te1550/dc_gap=200nm_Lc=Lum.sparam` |
| Ring coupler, gap *g* nm, radius *r* µm.dat (100 nm & 5 µm, 100 nm & 10 µm, 150 nm & 10 µm) | `klayout/EBeam/CML/EBeam/source_data/ebeam_dc_halfring_straight/te_ebeam_dc_halfring_straight_gap=gnm_radius=rnm_width=500nm_thickness=220nm_CoupleLength=0nm.dat` |

`tests/foundry.rs` checks each one at 1550 nm. Other corners of the process (thickness
210/230 nm, width 480/520 nm) and other components are in the PDK: import them into the
library the same way (File → Import into library…).
