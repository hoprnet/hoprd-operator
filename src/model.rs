/// All errors possible to occur during reconciliation
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Any error originating from the `kube-rs` crate
    #[error("Kubernetes reported error: {source}")]
    KubeError {
        #[from]
        source: kube::Error,
    },
    /// Error in user input or Hoprd resource definition, typically missing fields.
    #[error("Invalid Hoprd CRD: {0}")]
    UserInputError(String),

    #[error("IdentityHoprdPool error: {0}")]
    IdentityHoprdPoolError(String),

    #[error("IdentityHoprdPVC error: {0}")]
    IdentityHoprdPVCError(String),

    /// The hoprd is in an Unknown status
    #[error("Invalid Hoprd status: {0}")]
    HoprdStatusError(String),

    /// The hoprd configuration is invalid
    #[error("Invalid Hoprd configuration: {0}")]
    HoprdConfigError(String),

    #[error("YAML Parsing error: {0}")]
    ParserError(
        #[from]
        serde_yml::Error,
    ),

    #[error("JSON error: {0}")]
    JsonError(
        #[from]
        serde_json::Error,
    ),

    /// The hoprd configuration is invalid
    #[error("ClusterHoprd synch error: {0}")]
    ClusterHoprdSynchError(String),

    /// The Job execution did not complete successfully
    #[error("Job Execution failed: {0}")]
    JobExecutionError(String),

    /// The Job execution did not complete successfully
    #[error("This action is not supported: {0}")]
    OperationNotSupported(String),

    /// The Job execution did not complete successfully
    #[error("There is an issue with the identity: {0}")]
    IdentityIssue(String),

    /// A reconciliation panicked and was recovered
    #[error("Reconciler panic: {0}")]
    ReconcilerPanic(String),

    /// Waiting for a Kubernetes condition failed
    #[error("Wait error: {0}")]
    WaitError(
        #[from]
        kube::runtime::wait::Error,
    ),

    /// A field that the operator needs is not present in the resource or in the cluster object
    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_displayed_with_their_context() {
        assert_eq!(Error::MissingField("metadata.name".to_owned()).to_string(), "Missing required field: metadata.name");
        assert_eq!(Error::ReconcilerPanic("Hoprd".to_owned()).to_string(), "Reconciler panic: Hoprd");
        assert!(Error::UserInputError("bad".to_owned()).to_string().contains("bad"));
    }

    #[test]
    fn parser_and_json_errors_are_converted() {
        let yaml_error: Result<u8, Error> = serde_yml::from_str::<u8>("{").map_err(Error::from);
        assert!(matches!(yaml_error, Err(Error::ParserError(_))));
        let json_error: Result<u8, Error> = serde_json::from_str::<u8>("x").map_err(Error::from);
        assert!(matches!(json_error, Err(Error::JsonError(_))));
    }
}
