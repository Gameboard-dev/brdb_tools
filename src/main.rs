use brdb::{
    pending::BrPendingFs, schema::{
        BrdbSchema, 
        BrdbSchemaGlobalData
    }, BrReader, Brdb, ChunkIndex, Entity, EntityChunkIndexSoA, EntityChunkSoA, IntoReader
};

use std::{
    sync::Arc,
    env,
    fs,
    path::PathBuf,
};


/// Constructs the directory path for a world file in `'LOCALAPPDATA/Brickadia/Saved/Worlds/'`.
fn world_path(filename: &str) -> PathBuf {
    let local: String = env::var("LOCALAPPDATA").expect("Windows has no LOCALAPPDATA");
    PathBuf::from(format!("{}/Brickadia/Saved/Worlds/{}", local, filename))
}

fn is_dynamic_grid(entity: &Entity) -> bool {
    return entity.data.get_schema_struct().is_some_and(|s| s.0.as_ref() == "Entity_DynamicBrickGrid")
}

struct WorldProcessor {
    global_data: Arc<BrdbSchemaGlobalData>,
    entity_schema: Arc<BrdbSchema>,
    db: BrReader<Brdb>,
    pending: Vec<(String, BrPendingFs)>,
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
            pending: vec![]
        })
    }

    fn duplicate_entities(&mut self) -> Result<(), Box<dyn std::error::Error>> {

        let mut old_entity_soa: EntityChunkIndexSoA = self.db.entity_chunk_index_soa()?;
        let mut grid_ids = vec![];
        
        for chunk_index in old_entity_soa.chunk_3d_indices {
            
            let entities: Vec<Entity> = self.db.entity_chunk(chunk_index)?;
            let mut new_entity_soa = EntityChunkSoA::default();

            for mut entity in entities.into_iter() {
                
                let index: u32 = entity.id.unwrap() as u32;
                
                entity.frozen = true;
                new_entity_soa.add_entity(&self.global_data, &entity, index);

                let mut duplicate: Entity = entity.clone();
                duplicate.location.x += 200f32;

                new_entity_soa.add_entity(&self.global_data, &duplicate, old_entity_soa.next_persistent_index);
                old_entity_soa.next_persistent_index += 1;

                // Determine if the chunk is a dynamic brick grid
                // https://github.com/brickadia-community/brdb/blob/attempt-remove-shadows/crates/brdb/examples/world_remove_shadows.rs#L20-L23
                if is_dynamic_grid(&entity) {
                    if let Some(id) = entity.id {
                        grid_ids.push(id);
                    }
                }

            }

            let bytes: Vec<u8> = new_entity_soa.to_bytes(&self.entity_schema)?;

            self.pending.push((
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
                    ("Entities".to_string(),
                    BrPendingFs::Folder(Some(vec![
                        ("Chunks".to_string(),
                        // `pending` is moved and emptied
                        BrPendingFs::Folder(Some(std::mem::take(&mut self.pending))),
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

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut processor = WorldProcessor::new("Monastery.brdb")?;
    processor.duplicate_entities()?;
    processor.save_as("Monastery_V5.brdb")?;
    Ok(())
}