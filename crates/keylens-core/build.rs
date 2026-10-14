fn main() {
    // 数据文件变化时重新编译，让 include_dir! 打包最新的内置数据
    println!("cargo:rerun-if-changed=../../data/apps");
}
