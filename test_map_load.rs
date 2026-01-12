// Test script para verificar carga de mapas HaxBall
use std::fs;

fn main() {
    let map_path = "maps/futsal_bazinga.hbs";

    println!("🧪 Probando carga de mapa: {}", map_path);

    // Leer archivo
    match fs::read_to_string(map_path) {
        Ok(content) => {
            println!("✅ Archivo leído: {} bytes", content.len());

            // Intentar parsear como JSON
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => {
                    println!("✅ JSON válido");

                    if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
                        println!("   Nombre: {}", name);
                    }

                    if let Some(vertexes) = json.get("vertexes").and_then(|v| v.as_array()) {
                        println!("   Vértices: {}", vertexes.len());
                    }

                    if let Some(segments) = json.get("segments").and_then(|s| s.as_array()) {
                        println!("   Segmentos: {}", segments.len());
                    }

                    if let Some(discs) = json.get("discs").and_then(|d| d.as_array()) {
                        println!("   Discos: {}", discs.len());
                    }
                }
                Err(e) => {
                    println!("❌ Error parseando JSON: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ Error leyendo archivo: {}", e);
        }
    }
}
