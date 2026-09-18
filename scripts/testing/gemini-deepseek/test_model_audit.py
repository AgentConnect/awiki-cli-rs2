import unittest
from types import SimpleNamespace
from model_audit import route_record


class ModelAuditTest(unittest.TestCase):
    def test_only_routing_identifiers_are_recorded(self):
        value = route_record({'model':'deepseek-flash', 'messages':['private'],
            'api_key':'private', 'litellm_params':{'custom_llm_provider':'deepseek',
            'metadata':{'model_group':'deepseek-flash','authorization':'private'}}},
            SimpleNamespace(model='deepseek-flash', content='private'))
        self.assertEqual(value, {'requested':'deepseek-flash','model':'deepseek-flash',
            'provider':'deepseek','response_model':'deepseek-flash'})

    def test_missing_fields_are_unknown_without_fallback_claims(self):
        self.assertEqual(route_record({}, object()),dict.fromkeys(['requested','model','provider','response_model']))

    def test_google_metadata_takes_precedence(self):
        value = route_record({'litellm_params': {
            'metadata': {'model_group':'wrong'},
            'litellm_metadata': {'model_group':'deepseek-v4-pro','secret':'private'}}}, object())
        self.assertEqual(value['requested'], 'deepseek-v4-pro')
        self.assertNotIn('private', str(value))
