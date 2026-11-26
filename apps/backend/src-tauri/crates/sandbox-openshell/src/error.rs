//! `tonic::Status` → `SandboxError`（对齐上游文档 §8 的错误表）

use sandbox::SandboxError;

/// gRPC 状态码（protobuf 数值）→ 契约错误
pub(crate) fn status(op: &str, status: tonic::Status) -> SandboxError {
    let message = format!("{op}: {}", status.message());
    match status.code() as i32 {
        // Unauthenticated / PermissionDenied
        16 | 7 => SandboxError::auth(message),
        // NotFound
        5 => SandboxError::not_found(message),
        // AlreadyExists / Aborted / FailedPrecondition
        6 | 10 | 9 => SandboxError::conflict(message),
        // ResourceExhausted
        8 => SandboxError::rate_limited(message, None),
        // InvalidArgument / OutOfRange
        3 | 11 => SandboxError::invalid_request(400, message),
        // Cancelled
        1 => SandboxError::Cancelled,
        // Unimplemented
        12 => SandboxError::unsupported(message),
        // DeadlineExceeded（精确秒数未知，按 1s 占位）
        4 => SandboxError::timeout(1),
        // Unavailable / Internal / Unknown / DataLoss / Ok
        _ => SandboxError::server(502, message),
    }
}

/// 传输层错误（连接 / TLS / 建流）
#[allow(dead_code)]
pub(crate) fn transport(op: &str, error: impl std::fmt::Display) -> SandboxError {
    SandboxError::transport(format!("{op}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_codes_map_to_expected_kinds() {
        let cases = [
            (tonic::Code::Unauthenticated, "auth"),
            (tonic::Code::PermissionDenied, "auth"),
            (tonic::Code::NotFound, "not_found"),
            (tonic::Code::AlreadyExists, "conflict"),
            (tonic::Code::FailedPrecondition, "conflict"),
            (tonic::Code::ResourceExhausted, "rate_limited"),
            (tonic::Code::InvalidArgument, "invalid_request"),
            (tonic::Code::Cancelled, "cancelled"),
            (tonic::Code::Unimplemented, "unsupported"),
            (tonic::Code::DeadlineExceeded, "timeout"),
            (tonic::Code::Unavailable, "server"),
            (tonic::Code::Internal, "server"),
        ];
        for (code, expected) in cases {
            let error = status("test", tonic::Status::new(code, "boom"));
            assert_eq!(error.kind(), expected, "code={code:?}");
        }
    }
}