use brdb::{
    fs::BrFs, pending::BrPendingFs, schema::{
        BrdbSchema, 
        BrdbSchemaGlobalData
    }, BrReader, Brdb, Entity, EntityChunkIndexSoA, EntityChunkSoA, IntoReader
};

// Import the derive macro for BrFsReader if it is in a proc-macro crate
use brdb::BrFsReader;

use std::{
    env,
    mem,
    fs,
    sync::Arc,
    path::PathBuf,
};


/// Constructs the directory path for a world file in `'LOCALAPPDATA/Brickadia/Saved/Worlds/'`.
fn world_path(filename: &str) -> PathBuf {
    let local: String = env::var("LOCALAPPDATA").expect("Windows has no LOCALAPPDATA");
    PathBuf::from(format!("{}/Brickadia/Saved/Worlds/{}", local, filename))
}

fn is_dynamic_grid(entity: &Entity) -> bool {
    return entity.data.get_schema_struct()
    .is_some_and(|s| s.0.as_ref() == "Entity_DynamicBrickGrid")
}

struct Pending {
    entity_files: Vec<(String, BrPendingFs)>,
    grid_files: Vec<(String, BrPendingFs)>
}

impl Default for Pending {
    fn default() -> Self {
        Self {
            entity_files: Vec::new(),
            grid_files: Vec::new(),
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

        let mut src_entities: EntityChunkIndexSoA = self.db.entity_chunk_index_soa()?;

        let grids: BrFs = self.db.get_fs()?.cd("/World/0/Bricks/Grids")?;

        for chunk_index in src_entities.chunk_3d_indices {

            let entities: Vec<Entity> = self.db.entity_chunk(chunk_index)?;
            let mut dst_entities = EntityChunkSoA::default();

            for entity in entities.into_iter() {
                
                // Use original indexes
                let index: u32 = entity.id.unwrap() as u32;
                println!("Old Index {}", index);
                dst_entities.add_entity(&self.global_data, &entity, index);

                src_entities.next_persistent_index += 1; // Ensure unique entity indexing
                let new_index: u32 = src_entities.next_persistent_index;
                println!("New Index {}", new_index);

                // Duplicate lacks grid
                let mut duplicate: Entity = entity.clone();
                duplicate.location.x += 200f32;
                duplicate.id = Some(new_index as usize);
                
                dst_entities.add_entity(&self.global_data, &duplicate, new_index);

                // Determine if the chunk is a dynamic brick grid
                // https://github.com/brickadia-community/brdb/blob/attempt-remove-shadows/crates/brdb/examples/world_remove_shadows.rs#L20-L23

                if is_dynamic_grid(&entity) {
                    if let Some(grid_id) = entity.id {
                        let dir: BrPendingFs = grids.cd(grid_id.to_string())?.to_pending(&*self.db)?;

                        self.pending.grid_files.push((grid_id.to_string(), dir.clone()));
                        println!("Pushed Dynamic Grid {}", grid_id.to_string());
                        
                        self.pending.grid_files.push((new_index.to_string(), dir));
                        println!("Pushed Dynamic Grid {}", new_index.to_string())
                    }
                };
            };


            // Pushes updated dst_entities with duplicates
            let bytes: Vec<u8> = dst_entities.to_bytes(&self.entity_schema)?;

            self.pending.entity_files.push((
                format!("{chunk_index}.mps"),
                BrPendingFs::File(Some(bytes)),
            ));

        }

        Ok(())
    }


    fn patch(&mut self) -> BrPendingFs {
        return BrPendingFs::Root(vec![(
            "World".to_owned(),
            BrPendingFs::Folder(Some(vec![
                ("0".to_string(),
                BrPendingFs::Folder(Some(vec![
                    ("Bricks".to_string(), BrPendingFs::Folder(Some(vec![(
                        "Grids".to_string(),
                        BrPendingFs::Folder(Some(mem::take(&mut self.pending.grid_files))),
                    )]))),
                    ("Entities".to_string(), BrPendingFs::Folder(Some(vec![
                        ("Chunks".to_string(),
                        BrPendingFs::Folder(Some(mem::take(&mut self.pending.entity_files))),
                    )])),
                )])),
            )])),
        )]);
    }


    fn save_as(&mut self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        
        let pending: BrPendingFs = self.db.to_pending()?.with_patch(self.patch())?;
        let savefile: PathBuf = world_path(filename);

        if savefile.exists() {
            fs::remove_file(&savefile)?;
        }

        Brdb::new(&savefile)?.write_pending("Update", pending)?;
        println!("Succesfully saved {} to worlds folder in Brickadia", filename);

        Ok(())
    }

    #[allow(dead_code)]
    fn debug(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let grids: BrFs = self.db.get_fs()?.cd("/World")?;
        println!("[debug] {}", grids.render());
        Ok(())
    }

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut processor = WorldProcessor::new("Monastery.brdb")?;
    processor.duplicate_entities()?;
    processor.save_as("Monastery_V5.brdb")?;
    Ok(())
    
}