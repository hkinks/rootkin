// Builds the fictional story databases used by tests/migrate.rs with the real LiteDB 4.1.4 library
// (NuGet "LiteDB" 4.1.4). Not part of the Rust build; see docs/design.md ("Database-backed stories").
//   dotnet run -- <out-dir>
// Writes stories7 (password B, ~2 chunks), stories7.a (password A) and stories7.plain (no password).
using System.Text;
using LiteDB;

const string PasswordA = "5EuBD8Bfmx53Re9oyiEYvJO7AQF9dLLMbON2ZBMICzQeaRKAbUNE1JrUi22X518";
const string PasswordB = "gkBrhnC6uSzoU2KMOL4UeNfNpeIqSSRkEn0PliJJ5NJP3IJViZE9o1eAQw6dbQI";

string outDir = args[0];
Directory.CreateDirectory(outDir);

string Rtf(string body) => "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Segoe UI;}}\\f0\\fs22 " + body + "\\par}";

// Long story: several paragraphs, non-ASCII, > 255 KiB so that LiteDB stores two chunks and an extend page chain.
var big = new StringBuilder();
for (int i = 0; i < 2600; i++) big.Append($"Paragraph {i}: Åse walked to Köping past the mill, 水車 turned, naïve café ☕ \\u8212? and the river rose. ");
string bigRtf = Rtf(big.ToString());

void Make(string name, string? password, params (string id, string filename, string content)[] files)
{
    var path = Path.Combine(outDir, name);
    if (File.Exists(path)) File.Delete(path);
    var cs = password == null ? $"Filename={path}" : $"Filename={path}; Password={password}; EncryptionVersion=2";
    using var db = new LiteDatabase(cs);
    foreach (var (id, filename, content) in files)
    {
        using var ms = new MemoryStream(new UTF8Encoding(false).GetBytes(content));
        db.FileStorage.Upload(id, filename, ms);
    }
}

Make("stories7", PasswordB,
    ("11111111-aaaa-4bbb-8ccc-000000000001", "ada.rtf", Rtf("Ada grew up at the rectory in Dunmow. \\u197?se, K\\u246?ping, caf\\u233?.")),
    ("11111111-aaaa-4bbb-8ccc-000000000002", "ada - annotations.xml", "<Annotations xmlns=\"http://schemas.microsoft.com/windows/annotations/2003/11/core\"><Annotation Id=\"x\" /></Annotations>"),
    ("11111111-aaaa-4bbb-8ccc-000000000003", "william.txt", "William was a surveyor.\r\nUnicode: Åse, Köping, 水車, naïve café ☕, 𝄞.\r\nPlain text kept as is."),
    ("11111111-aaaa-4bbb-8ccc-000000000004", "anne.rtf", bigRtf),
    ("11111111-aaaa-4bbb-8ccc-000000000005", "empty.rtf", ""));
Make("stories7.a", PasswordA,
    ("22222222-aaaa-4bbb-8ccc-000000000001", "hugh.rtf", Rtf("Hugh stood godfather to Anne.")));
Make("stories7.plain", null,
    ("33333333-aaaa-4bbb-8ccc-000000000001", "hugh.rtf", Rtf("Unprotected database.")));
