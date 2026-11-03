// Test de raccord (temporaire) : tuile décalée d'une demi-période → une couture
// éventuelle apparaît en croix au centre de l'image.
#include "procgen/texgen.slang"
#include "lib/rock.slang"
#include "lib/wrap.slang"

Surface surface(float2 p, uint seed) {
  float2 q = frac(p + 0.5);
  float h = terraces(q, 5, 0.05, 0.02, 0.025, 0.02, seed).height
            + terraces(q, 13, 0.02, 0.008, 0.008, 0.03, seed + 100u).height;
  Surface s = { h, float3(saturate(h * 30.0 + 0.3)) };
  return s;
}
