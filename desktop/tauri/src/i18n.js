/* WristKey i18n — loaded as global script, not ES module */
window.WristKeyI18n = (function() {
    let currentLang = localStorage.getItem('wristkey-lang') || 'en';
    let translations = {};

    async function loadLanguage(lang) {
        try {
            const response = await fetch(`i18n/${lang}.json`);
            if (!response.ok) throw new Error(`Failed to load ${lang}.json`);
            translations = await response.json();
            currentLang = lang;
            localStorage.setItem('wristkey-lang', lang);
            document.documentElement.lang = lang;
            applyTranslations();
            return true;
        } catch (e) {
            console.error('i18n load error:', e);
            return false;
        }
    }

    function t(key, params) {
        params = params || {};
        let text = translations[key] || key;
        if (Array.isArray(text)) {
            text = text.join('\n');
        }
        Object.keys(params).forEach(function(k) {
            text = text.replace(new RegExp('\\{' + k + '\\}', 'g'), params[k]);
        });
        return text;
    }

    function applyTranslations() {
        document.querySelectorAll('[data-i18n]').forEach(function(el) {
            var key = el.getAttribute('data-i18n');
            var text = t(key);
            if (el.tagName === 'INPUT' && el.type !== 'button') {
                el.placeholder = text;
            } else {
                el.textContent = text;
            }
        });
        document.querySelectorAll('[data-i18n-placeholder]').forEach(function(el) {
            var key = el.getAttribute('data-i18n-placeholder');
            el.placeholder = t(key);
        });
    }

    function getCurrentLang() {
        return currentLang;
    }

    function init() {
        return loadLanguage(currentLang);
    }

    return {
        loadLanguage: loadLanguage,
        t: t,
        getCurrentLang: getCurrentLang,
        init: init,
        applyTranslations: applyTranslations
    };
})();
