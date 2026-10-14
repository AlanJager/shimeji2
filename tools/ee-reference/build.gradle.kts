plugins {
    java
    application
}

java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}

repositories {
    mavenCentral()
    maven { url = uri("https://jitpack.io") }
}

dependencies {
    implementation("org.openjdk.nashorn:nashorn-core:15.7")
    implementation("org.slf4j:slf4j-api:2.0.20")
    runtimeOnly("org.slf4j:slf4j-jdk14:2.0.20")
    implementation("net.java.dev.jna:jna:5.19.1")
    implementation("net.java.dev.jna:jna-platform:5.19.1")
    implementation("org.apache.commons:commons-exec:1.6.0")
    implementation("com.formdev:flatlaf:3.7.2")
    // jfa 1.2.0 depends on jna with the jpms classifier. JNA 5.19.1 no longer
    // publishes that classifier (the main jar is the modular one). The harness
    // never calls OsThemeDetector, so the Mac theme bridge is not on the classpath.
    implementation("com.github.Dansoftowner:jSystemThemeDetector:3.9.1") {
        exclude(group = "de.jangassen", module = "jfa")
    }
}

application {
    mainClass.set("shimeji2.ref.TraceRunner")
}

sourceSets {
    main {
        java {
            srcDir("src/main/java")
            srcDir("vendor/src/main/java")
            exclude("module-info.java")
        }
        resources {
            srcDir("vendor/src/main/resources")
        }
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
    options.release.set(21)
}

tasks.register("printClasspath") {
    val cp = sourceSets.main.get().runtimeClasspath
    val outFile = layout.buildDirectory.file("runtime-classpath.txt")
    dependsOn(tasks.named("classes"))
    doLast {
        outFile.get().asFile.writeText(cp.asPath)
    }
}

tasks.named<JavaExec>("run") {
    workingDir = layout.projectDirectory.dir("run/shime").asFile
    systemProperty("shimeji2.ref.factory", "headless")
    systemProperty("file.encoding", "UTF-8")
}

tasks.register<JavaExec>("dumpVectors") {
    group = "shimeji2"
    description = "Write java.util.Random and Nashorn expression vectors for shimeji-expr tests."
    classpath = sourceSets.main.get().runtimeClasspath
    mainClass.set("shimeji2.ref.ExprVectors")
    systemProperty("file.encoding", "UTF-8")
    val repo = rootProject.projectDir.parentFile.parentFile
    args(
        "--shime-conf", repo.resolve("assets/packs/shime/conf").absolutePath,
        "--out-nashorn", repo.resolve("crates/shimeji-expr/tests/vectors/nashorn.json").absolutePath,
        "--out-random", repo.resolve("crates/shimeji-expr/tests/vectors/java_random.json").absolutePath,
    )
}
