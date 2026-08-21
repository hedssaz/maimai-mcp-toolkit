use super::dto::TokenStatusDto;

pub const fn token_bound() -> &'static str {
    "Developer-Token 已绑定。"
}

pub fn token_status(status: &TokenStatusDto) -> String {
    match status.updated_at.as_deref() {
        Some(updated_at) if status.bound => format!(
            "Developer-Token 已绑定。\n更新时间：{updated_at}\n安全提示：{}",
            status.security_notice
        ),
        _ if status.bound => format!(
            "Developer-Token 已绑定。\n安全提示：{}",
            status.security_notice
        ),
        _ => format!(
            "Developer-Token 未绑定。\n安全提示：{}",
            status.security_notice
        ),
    }
}

pub const fn token_cleared(cleared: bool) -> &'static str {
    if cleared {
        "Developer-Token 已清除。"
    } else {
        "没有已绑定的 Developer-Token。"
    }
}

pub fn source_switched(source_label: &str, lxns_allowed: bool) -> String {
    if lxns_allowed {
        format!(
            "已切换成绩默认数据源：{source_label}。\n后续查询只会使用该数据源，不会自动切换到另一个数据源。\n通常使用水鱼；如果已完成落雪 OAuth，可切到落雪；如果不想绑定外部查分器，可以切到本地缓存。\n本地缓存需要先通过本机器人完成成绩导入。\n之后可发送 source sy、source lxns 或 source local 再切换。"
        )
    } else {
        format!(
            "已切换成绩默认数据源：{source_label}。\n后续查询只会使用该数据源，不会自动切换到另一个数据源。\n通常使用水鱼；如果不想绑定外部查分器，可以切到本地缓存。\n本地缓存需要先通过本机器人完成成绩导入。\n之后可发送 source sy 或 source local 再切换。"
        )
    }
}
