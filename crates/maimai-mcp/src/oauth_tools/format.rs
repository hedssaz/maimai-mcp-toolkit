use super::dto::{ConfirmDto, StatusDto, UnbindDto};

pub fn authorization_url(url: &str) -> String {
    format!(
        "打开下面的落雪授权链接完成授权：\n{url}\n授权完成后提交回调 code；拍一拍流程还需要原上下文确认。"
    )
}

pub const fn bound() -> &'static str {
    "落雪 OAuth 已绑定。"
}

pub const fn pending() -> &'static str {
    "已收到落雪授权，等待原用户在原会话拍一拍当前机器人确认。"
}

pub fn confirmed(result: &ConfirmDto) -> &'static str {
    match result.status.as_str() {
        "confirmed" => "落雪 OAuth 已确认绑定。",
        "context_mismatch" => "待确认绑定与当前用户、适配器、会话或机器人不匹配。",
        "expired" => "待确认绑定已过期，请重新授权。",
        "not_found" => "没有找到待确认的落雪 OAuth 绑定。",
        _ => "落雪 OAuth 确认未完成。",
    }
}

pub fn status(result: &StatusDto) -> &'static str {
    if result.bound {
        "落雪 OAuth：已绑定。"
    } else if result.pending {
        "落雪 OAuth：等待拍一拍确认。"
    } else {
        "落雪 OAuth：未绑定。"
    }
}

pub fn unbind(result: &UnbindDto) -> &'static str {
    if result.changed {
        "落雪 OAuth 已解绑。"
    } else {
        "当前没有已绑定的落雪 OAuth。"
    }
}
