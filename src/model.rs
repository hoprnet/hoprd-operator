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

    /// The secret is in an Unknown status
    #[error("Invalid Hoprd Secret status: {0}")]
    IdentityStatusError(String),

    /// The hoprd is in an Unknown status
    #[error("Invalid Hoprd status: {0}")]
    HoprdStatusError(String),

    /// The hoprd configuration is invalid
    #[error("Invalid Hoprd configuration: {0}")]
    HoprdConfigError(String),

    #[error("YAML Parsing error: {0}")]
    ParserError(
        #[from]
        serde_yaml::Error,
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
}
