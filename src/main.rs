use brdb::{
    Entity,
    EntityChunkSoA,
    ChunkIndex,
    schema::{
        BrdbSchema, 
        BrdbSchemaGlobalData
    },
    BrReader,
    Brdb,
    IntoReader,
    pending::{
        BrPendingFs
    }
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

        let entity_chunk_indices: Vec<ChunkIndex> = self.db.entity_chunk_index()?;
        let mut new_index: u32 = 500;
        
        for chunk_index in entity_chunk_indices {
            let entities: Vec<Entity> = self.db.entity_chunk(chunk_index)?;
            let mut structure_of_arrays = EntityChunkSoA::default();

            for mut entity in entities.into_iter() {
                let index: u32 = entity.id.unwrap() as u32;
                entity.frozen = true;
                structure_of_arrays.add_entity(&self.global_data, &entity, index);

                let mut duplicate: Entity = entity.clone();
                duplicate.location.x += 200f32;
                structure_of_arrays.add_entity(&self.global_data, &duplicate, new_index);
                new_index += 1;
            }

            let bytes: Vec<u8> = structure_of_arrays.to_bytes(&self.entity_schema)?;

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