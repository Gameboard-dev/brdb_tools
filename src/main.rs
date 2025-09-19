use brdb::{
    byte_to_orientation, fs::BrFs, pending::BrPendingFs, schema::{
        BrdbSchema, BrdbSchemaGlobalData, BrdbStruct, BrdbValue
    }, schemas::ENTITY_CHUNK_INDEX_SOA, BString, BitFlags, BrFsReader, BrReader, Brdb, BrdbSchemaError, Brick, BrickChunkSoA, BrickType, ChunkIndex, ChunkMeta, Collision, Color, Entity, EntityChunkIndexSoA, EntityChunkSoA, IntoReader, Position, RelativePosition, World
};


use itertools::izip;

use std::{
    env,
    mem,
    fs,
    sync::Arc,
    path::PathBuf,
    fs::File,
    io::Write,
    convert::TryInto
};


/// Constructs the directory path for a world file in `'LOCALAPPDATA/Brickadia/Saved/Worlds/'`.
fn world_path(filename: &str) -> PathBuf {
    let local: String = env::var("LOCALAPPDATA").expect("Windows has no LOCALAPPDATA");
    PathBuf::from(format!("{}/Brickadia/Saved/Worlds/{}", local, filename))
}

/// Determine if the chunk is a dynamic brick grid
/// https://github.com/brickadia-community/brdb/blob/attempt-remove-shadows/crates/brdb/examples/world_remove_shadows.rs#L20-L23
fn is_dynamic_grid(entity: &Entity) -> bool {
    return entity.data.get_schema_struct()
    .is_some_and(|s| s.0.as_ref() == "Entity_DynamicBrickGrid")
}

struct Pending {
    entity_files: Vec<(String, BrPendingFs)>,
    grid_files: Vec<(String, BrPendingFs)>,
    chunk_index: Vec<u8>
}

impl Default for Pending {
    fn default() -> Self {
        Self {
            entity_files: Vec::new(),
            grid_files: Vec::new(),
            chunk_index: Vec::new()
        }
    }
}

const GLOBAL_GRID_ID:usize = 1;
const TEST_BRICK_OFFSET_Z:i32 = 200;


struct WorldProcessor {
    global_data: Arc<BrdbSchemaGlobalData>,
    entity_schema: Arc<BrdbSchema>,
    db: BrReader<Brdb>,
    pending: Pending,
}


impl WorldProcessor {

    fn new(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file: PathBuf = world_path(filename);
        assert!(file.exists(), "File does not exist: {:?}", file);
        let db: BrReader<Brdb> = Brdb::open(file)?.into_reader();
        Ok(Self {
            global_data: db.global_data()?,
            entity_schema: db.entities_schema()?,
            db,
            pending: Pending::default(),
        })
    }

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

                let mut position = Position::from_relative(chunk_index, relative_position);
                position.z += TEST_BRICK_OFFSET_Z;

                world.bricks.push(Brick {
                    asset: metadata.brick_type_by_index(
                                        type_index, 
                                        chunk.procedural_brick_starting_index, 
                                        brick_size, 
                                        size_counter
                                    )?,
                    owner_index: Some(owner_index as usize),
                    position, 
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

        let pending: BrPendingFs = world.to_unsaved()?.to_pending()?;
        let brick_grid: &BrPendingFs = pending.cd(format!("/World/0/Bricks/Grids/{GLOBAL_GRID_ID}"))?;
        self.pending.grid_files.push((GLOBAL_GRID_ID.to_string(), brick_grid.clone()));

        return Ok(pending)
    
    }

    fn quadruple(&mut self) -> Result<(), Box<dyn std::error::Error>> {

        let mut entity_chunk_index_soa: EntityChunkIndexSoA = self.db.entity_chunk_index_soa()?;

        let grids: BrFs = self.db.get_fs()?.cd("/World/0/Bricks/Grids")?;

        for &chunk_index in entity_chunk_index_soa.chunk_3d_indices.iter() {

            let entities: Vec<Entity> = self.db.entity_chunk(chunk_index)?;
            let mut entity_chunk_soa = EntityChunkSoA::default();

            for entity in entities.into_iter() {
                
                // Use original indexes
                let grid_id: usize = entity.id.unwrap();
                //println!("Old Index {}", grid_id);
                entity_chunk_soa.add_entity(&self.global_data, &entity, grid_id as u32);

                let mut brick_grid:Option<BrPendingFs> = None;

                if is_dynamic_grid(&entity) {
                    brick_grid = Some(grids.cd(grid_id.to_string())?.to_pending(&*self.db)?);
                    self.pending.grid_files.push((grid_id.to_string(), brick_grid.clone().unwrap()));
                    //println!("Pushed Dynamic Grid {}", grid_id.to_string());
                };

                let mut duplicates = vec![];

                let num_columns = 4; 
                let num_rows = 4;  

                for col in 0..num_columns {
                    for row in 0..num_rows {

                        if row == 0 && col == 0 {
                            continue;
                        }

                        let mut duplicate: Entity = entity.clone();

                        let add_x = 200.0 * col as f32;
                        let add_y = 200.0 * row as f32; 

                        duplicate.location.x += add_x;
                        duplicate.location.y += add_y;

                        let persistent_index: u32 = entity_chunk_index_soa.next_persistent_index;
                        duplicate.id = Some(persistent_index as usize);
                        //println!("New Index {}", persistent_index);

                        entity_chunk_soa.add_entity(&self.global_data, &duplicate, persistent_index);

                        if let Some(path) = brick_grid.clone() {
                            self.pending.grid_files.push((persistent_index.to_string(), path));
                            //println!("Pushed Dynamic Grid {}", persistent_index.to_string());
                        };

                        duplicates.push(duplicate);

                        entity_chunk_index_soa.num_entities[0] += 1;
                        entity_chunk_index_soa.next_persistent_index += 1;
                    }
                }

            };

            let serialized_entity: Vec<u8> = entity_chunk_soa.to_bytes(&self.entity_schema)?;

            self.pending.entity_files.push((
                format!("{chunk_index}.mps"),
                BrPendingFs::File(Some(serialized_entity)),
            ));

        }

        let chunk_index_bytes: Vec<u8> = self.db.entities_chunk_index_schema()?.write_brdb(ENTITY_CHUNK_INDEX_SOA, &entity_chunk_index_soa)?;
        self.pending.chunk_index = chunk_index_bytes;

        Ok(())
    }


    fn patch(&mut self) -> BrPendingFs {
        BrPendingFs::Root(vec![
            (
                "World".to_owned(),
                BrPendingFs::Folder(Some(vec![
                    (
                        "0".to_string(),
                        BrPendingFs::Folder(Some(vec![
                            (
                                "Bricks".to_string(),
                                BrPendingFs::Folder(Some(vec![
                                    (
                                        "Grids".to_string(),
                                        BrPendingFs::Folder(Some(mem::take(&mut self.pending.grid_files))),
                                    ),
                                ])),
                            ),
                            (
                                "Entities".to_string(),
                                BrPendingFs::Folder(Some(vec![
                                    (
                                        "Chunks".to_string(),
                                        BrPendingFs::Folder(Some(mem::take(&mut self.pending.entity_files))),
                                    ),
                                    (
                                        "ChunkIndex.mps".to_string(),
                                        BrPendingFs::File(Some(mem::take(&mut self.pending.chunk_index))),
                                    ),
                                ])),
                            ),
                        ])),
                    ),
                ])),
            ),
        ])
    }


    fn save_as(&mut self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        
        let pending: BrPendingFs = self.db.to_pending()?.with_patch(self.patch())?;
        let savefile: PathBuf = world_path(filename);

        if savefile.exists() {
            fs::remove_file(&savefile)?;
        }

        let new_db: Brdb = Brdb::new(&savefile)?;
        new_db.write_pending("Update", pending)?;
        self.db = new_db.into_reader();

        println!("Succesfully saved {} to worlds folder in Brickadia", filename);

        Ok(())
    }

    #[allow(dead_code)]
    fn debug(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let grids: BrFs = self.db.get_fs()?; //.cd("/World")?;
        let output = grids.render();

        let mut file = File::create("debug.txt")?;
        writeln!(file, "{}", output)?;

        Ok(())
    }

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut processor = WorldProcessor::new("Monastery.brdb")?;
    //processor.quadruple()?;
    let _ = processor.parse_world_grid();
    //let _ = processor.save_as("Monastery_Modified.brdb");
    //let _ = processor.debug();
    Ok(())
}