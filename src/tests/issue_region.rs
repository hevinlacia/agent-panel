use super::*;

/// issue 家族 region 区域维度：meta frontmatter 写入 → 扫描读取 roundtrip；
/// 规范化（大小写/空白）；普通需求不写 region。
#[tokio::test]
async fn region_roundtrip_via_meta_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("WMS-INC-001-demo");
    std::fs::create_dir_all(&dir).unwrap();
    let meta = build_meta_doc(
        "WMS-INC-001-demo",
        "示例线上问题",
        "排查中",
        "WMS",
        &["WMS".to_string()],
        "线上问题",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "unknown",
        "",
        &[],
        "示例",
        None,
        None,
    );
    std::fs::write(dir.join("meta.md"), &meta).unwrap();
    // 模拟创建链路：region 就地写入 meta frontmatter（与 create.rs 落盘前修改一致）。
    let mut files_body = std::fs::read_to_string(dir.join("meta.md")).unwrap();
    files_body = set_frontmatter_field(&files_body, "region", "SEA"); // 大写输入
    std::fs::write(dir.join("meta.md"), files_body).unwrap();

    let req = load_requirement_from_dir(&dir, "WMS-INC-001-demo", &["WMS".to_string()], &[])
        .await
        .unwrap()
        .expect("requirement loads");
    assert_eq!(req.region.as_deref(), Some("sea")); // 读取时小写规范化
    assert_eq!(req.category.as_deref(), Some("线上问题"));
}

#[tokio::test]
async fn region_absent_means_none_and_patch_clear_removes_field() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("WMS-INC-002-demo");
    std::fs::create_dir_all(&dir).unwrap();
    let meta = build_meta_doc(
        "WMS-INC-002-demo",
        "未登记区域的问题",
        "排查中",
        "WMS",
        &["WMS".to_string()],
        "线上问题",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "unknown",
        "",
        &[],
        "示例",
        None,
        None,
    );
    std::fs::write(dir.join("meta.md"), &meta).unwrap();
    let req = load_requirement_from_dir(&dir, "WMS-INC-002-demo", &["WMS".to_string()], &[])
        .await
        .unwrap()
        .expect("requirement loads");
    assert!(req.region.is_none(), "未登记 region 应为 None");

    // PATCH 清空语义：set_frontmatter_field 空值移除字段。
    let with_region = set_frontmatter_field(&meta, "region", "cn");
    assert!(with_region.contains("region: cn"));
    let cleared = set_frontmatter_field(&with_region, "region", "");
    assert!(!cleared.contains("region:"), "空值应移除 region 字段");
    // 再写回仍有效（可反复修正）。
    assert!(set_frontmatter_field(&cleared, "region", "sea").contains("region: sea"));
}
