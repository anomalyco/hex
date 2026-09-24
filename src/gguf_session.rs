//! The `transcribe-cpp` session owner shared by the macOS and Linux GGUF
//! transcribers. It recreates the native session around every run so retained
//! scheduler scratch never outlives the inference that sized it.

pub(crate) struct OfflineGgufSession {
    model: transcribe_cpp::Model,
    session: Option<transcribe_cpp::Session>,
}

impl OfflineGgufSession {
    pub(crate) fn new(model: transcribe_cpp::Model) -> transcribe_cpp::Result<Self> {
        let session = model.session()?;
        Ok(Self {
            model,
            session: Some(session),
        })
    }

    pub(crate) fn run(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
    ) -> transcribe_cpp::Result<transcribe_cpp::Transcript> {
        // transcribe-cpp 0.1.x retains input-sized scheduler buffers for the
        // session lifetime. Drop each used session before creating its
        // successor so a long inference cannot pin that high-water mark or
        // overlap two sessions' persistent decoder state.
        let mut session = match self.session.take() {
            Some(session) => session,
            None => self.model.session()?,
        };
        let result = session.run(samples, options);
        drop(session);
        match self.model.session() {
            Ok(session) => {
                self.session = Some(session);
                result
            }
            Err(recovery_error) => match result {
                Ok(_) => Err(recovery_error),
                Err(run_error) => Err(run_error),
            },
        }
    }
}
