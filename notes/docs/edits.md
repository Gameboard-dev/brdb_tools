```rust

// brdb\crates\brdb\src\wrapper\brick.rs


impl TryFrom<&BrdbValue> for BrickSizeCounter {
    type Error = BrdbSchemaError;
    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            asset_index: value.prop("AssetIndex")?.as_brdb_u32()?,
            num_sizes: value.prop("NumSizes")?.as_brdb_u32()?,
        })
    }
}

impl TryFrom<&BrdbValue> for BrickSize {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_u16()?,
            y: value.prop("Y")?.as_brdb_u16()?,
            z: value.prop("Z")?.as_brdb_u16()?,
        })
    }
}

impl TryFrom<&BrdbValue> for RelativePosition {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_i16()?,
            y: value.prop("Y")?.as_brdb_i16()?,
            z: value.prop("Z")?.as_brdb_i16()?,
        })
    }
}


impl TryFrom<&BrdbValue> for (u8, u8, u8, u8) {
    type Error = BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        match value {
            BrdbValue::Array(items) | BrdbValue::FlatArray(items) if items.len() == 4 => {
                let r: u8 = (&items[0]).try_into()?;
                let g: u8 = (&items[1]).try_into()?;
                let b: u8 = (&items[2]).try_into()?;
                let a: u8 = (&items[3]).try_into()?;
                Ok((r, g, b, a))
            }
            _ => Err(BrdbSchemaError::ExpectedType(
                "Color Tuple".into(),
                value.get_type().into(),
            )),
        }
    }
}

impl TryFrom<&BrdbValue> for BrickChunkSoA {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            procedural_brick_starting_index: value.prop("ProceduralBrickStartingIndex")?.as_brdb_u32()?,
            brick_size_counters: value.prop("BrickSizeCounters")?.try_into()?,
            brick_sizes: value.prop("BrickSizes")?.try_into()?,
            brick_type_indices: value.prop("BrickTypeIndices")?.try_into()?,
            owner_indices: value.prop("OwnerIndices")?.try_into()?,
            relative_positions: value.prop("RelativePositions")?.try_into()?,
            orientations: value.prop("Orientations")?.try_into()?,
            collision_flags_player: value.prop("CollisionFlagsPlayer")?.as_brdb_bitflags()?,
            collision_flags_weapon: value.prop("CollisionFlagsWeapon")?.as_brdb_bitflags()?,
            collision_flags_interaction: value.prop("CollisionFlagsInteraction")?.as_brdb_bitflags()?,
            collision_flags_tool: value.prop("CollisionFlagsTool")?.as_brdb_bitflags()?,
            visibility_flags: value.prop("VisibilityFlags")?.as_brdb_bitflags()?,
            material_indices: value.prop("MaterialIndices")?.try_into()?,
            colors_and_alphas: value.prop("ColorsAndAlphas")?.try_into()?,
            num_brick_sizes: value.prop("NumBrickSizes")?.try_into()?,
            ..Default::default() // size_index_map
        })
    }
}

// global_data.rs

// Added new helper utilities here

impl BrdbSchemaGlobalData {

    pub fn get_material(&self, material_index: u8) -> Result<BString, BrdbSchemaError> {
        self
            .material_asset_names
            .get_index(material_index as usize)
            .map(|s| BString::Owned(s.to_owned()))
            .ok_or_else(|| BrdbSchemaError::InvalidMaterialIndex(material_index))
    }
    
    pub fn brick_type_by_index(
        &self,
        type_index: u32,
        procedural_brick_starting_index: u32,
        brick_size: BrickSize,
        size_counter: BrickSizeCounter,
    ) -> Result<BrickType, BrdbSchemaError> {
        if type_index < procedural_brick_starting_index {
            // Basic
            // Synonymous with number of basic assets
            // Uses the index directly to lookup the name
            // Related: 'BrickChunkSoA::add_brick'
            self.basic_brick_asset_names
                .get_index(type_index as usize)
                .map(|name| BrickType::Basic(BString::Owned(name.to_owned())))
                .ok_or_else(|| BrdbSchemaError::InvalidBrickTypeIndex(type_index))
        } else {
            // Procedural
            // Indexed after basic assets
            // Uses the BrickSizeCounter index value
            // Has a corresponding size
            self.procedural_brick_asset_names
                .get_index(size_counter.asset_index as usize)
                .map(|name| BrickType::Procedural {
                    asset: BString::Owned(name.to_owned()),
                    size: brick_size,
                })
                .ok_or_else(|| BrdbSchemaError::InvalidProceduralAssetIndex(size_counter.asset_index))
        }
    }
    
}


// Added 3 new errors

#[derive(Debug, Error)] 
pub enum BrdbSchemaError {
    #[error("Invalid material index: {0}")]
    InvalidMaterialIndex(u8),
    #[error("Invalid brick asset type index: {0}")]
    InvalidBrickTypeIndex(u32),
    #[error("Invalid procedural brick asset type index: {0}")]
    InvalidProceduralAssetIndex(u32)
}







// src > main > impl WorldProcessor


    fn parse_world_grid(&mut self) -> Result<BrPendingFs, Box<dyn std::error::Error>> {

        // This example ONLY parses the Global (static) Brick Grid
        // See https://github.com/brickadia-community/brdb/blob/main/crates/brdb/examples/write_entity.rs
        // For parsing all entity grids as well
        
        let mut world = World::new();

        let metadata: Arc<BrdbSchemaGlobalData> = self.db.read_global_data()?;
        let chunk_indices: Vec<ChunkMeta> = self.db.brick_chunk_index(GLOBAL_GRID_ID)?;
    
        for (i, chunk_meta) in chunk_indices.iter().enumerate() {

            let chunk_index: ChunkIndex = chunk_meta.index;
            let chunk: BrickChunkSoA = self.db.brick_chunk_soa(GLOBAL_GRID_ID, chunk_index)?.to_value().try_into()?;

            for (i, (
                relative_position, 
                orientation_byte, 
                rgba,
                material_index,
                type_index,
                size_counter,
                brick_size,
                owner_index,
            )) in izip!(
                chunk.relative_positions,
                chunk.orientations,
                chunk.colors_and_alphas,
                chunk.material_indices,
                chunk.brick_type_indices,
                chunk.brick_size_counters,
                chunk.brick_sizes,
                chunk.owner_indices,
            ).enumerate() {

                let (direction, rotation) = byte_to_orientation(orientation_byte);

                world.bricks.push(Brick {
                    asset: metadata.brick_type_by_index(
                                        type_index, 
                                        chunk.procedural_brick_starting_index, 
                                        brick_size, 
                                        size_counter
                                    )?,
                    owner_index: Some(owner_index as usize),
                    position: Position::from_relative(chunk_index, relative_position),
                    rotation,
                    direction,
                    collision: Collision {
                        player: chunk.collision_flags_player.get(i),
                        weapon: chunk.collision_flags_weapon.get(i),
                        interact: chunk.collision_flags_interaction.get(i),
                        tool: chunk.collision_flags_tool.get(i),
                    },
                    visible: chunk.visibility_flags.get(i),
                    color: Color::new(rgba.0, rgba.1, rgba.2),
                    material_intensity: rgba.3,
                    material: metadata.material_by_index(material_index)?,
                    components: Default::default(),
                    ..Default::default()
                    }.with_id()

                );

            };
        }

        return Ok(world.to_unsaved()?.to_pending()?)
    
    }

```