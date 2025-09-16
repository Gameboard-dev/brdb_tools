use brdb::{
    fs::BrFs, pending::BrPendingFs, schema::{
        BrdbSchema, 
        BrdbSchemaGlobalData
    }, schemas::ENTITY_CHUNK_INDEX_SOA, BrReader, Brdb, Entity, EntityChunkIndexSoA, EntityChunkSoA, IntoReader
};

// Import the derive macro for BrFsReader if it is in a proc-macro crate
use brdb::BrFsReader;

use std::{
    env,
    mem,
    fs,
    sync::Arc,
    path::PathBuf,
    fs::File,
    io::Write,
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

    fn duplicate_entities(&mut self) -> Result<(), Box<dyn std::error::Error>> {

        let mut entity_chunk_index_soa: EntityChunkIndexSoA = self.db.entity_chunk_index_soa()?;

        let grids: BrFs = self.db.get_fs()?.cd("/World/0/Bricks/Grids")?;

        for &chunk_index in entity_chunk_index_soa.chunk_3d_indices.iter() {

            let entities: Vec<Entity> = self.db.entity_chunk(chunk_index)?;
            let mut entity_chunk_soa = EntityChunkSoA::default();

            for entity in entities.into_iter() {
                
                // Use original indexes
                let grid_id: usize = entity.id.unwrap();
                println!("Old Index {}", grid_id);
                entity_chunk_soa.add_entity(&self.global_data, &entity, grid_id as u32);


                let mut brick_grid_path:Option<BrPendingFs> = None;

                if is_dynamic_grid(&entity) {
                    brick_grid_path = Some(grids.cd(grid_id.to_string())?.to_pending(&*self.db)?);
                    self.pending.grid_files.push((grid_id.to_string(), brick_grid_path.clone().unwrap()));
                    println!("Pushed Dynamic Grid {}", grid_id.to_string());
                };
                
                let mut duplicates = vec![];

                let num_columns = 2; 
                let num_rows = 2;  

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
                        println!("New Index {}", persistent_index);

                        entity_chunk_soa.add_entity(&self.global_data, &duplicate, persistent_index);

                        if let Some(path) = brick_grid_path.clone() {
                            self.pending.grid_files.push((persistent_index.to_string(), path));
                            println!("Pushed Dynamic Grid {}", persistent_index.to_string());
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
    processor.duplicate_entities()?;
    let _ = processor.save_as("Monastery_Modified.brdb");
    //let _ = processor.debug();
    Ok(())
}