package local.hex.bench;

import java.io.File;
import java.util.*;
import ly.anoma.hex.keyboard.KeyboardLetters;
import ly.anoma.hex.keyboard.LetterFeedback;
import ly.anoma.hex.typing.TypingModel;

/** Export the production model vocabulary and its hold-only letters on the actual layout. */
public final class GateLexicon {
    public static void main(String[] args) throws Exception {
        String lang = args[1];
        TypingModel model = TypingModel.Companion.file(new File(args[0]), lang);
        String keys = String.join("", KeyboardLetters.INSTANCE.rows(lang));
        List<String> holds = new ArrayList<>();
        for (char key : keys.toCharArray()) {
            for (String option : LetterFeedback.Companion.options(key, lang)) {
                char c = option.charAt(0);
                if (Character.isLetter(c) && keys.indexOf(c) == -1 && model.code(c) != 0)
                    holds.add(option + ":" + key);
            }
        }
        System.out.println(String.join(" ", holds));
        for (int i = 0; i < model.getWords(); i++) System.out.println(model.text(i));
    }
}
