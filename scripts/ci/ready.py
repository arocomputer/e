"""Require successful selected work and explicitly skipped unselected work."""
import json
import os
from changes import JOBS


def verify(needs):
    if needs['changes']['result'] != 'success':
        raise ValueError('change selection did not succeed')
    plan = json.loads(needs['changes']['outputs']['plan'])
    if set(plan) != set(JOBS) or any(type(value) is not bool for value in plan.values()):
        raise ValueError('invalid job selection')
    if set(needs) != {'changes', *JOBS}:
        raise ValueError('gate dependencies do not match selection')
    for job, selected in plan.items():
        expected = 'success' if selected else 'skipped'
        if needs[job]['result'] != expected:
            raise ValueError(f"{job}: expected {expected}, got {needs[job]['result']}")
        print(f"{job}: {expected}")


if __name__ == '__main__':
    verify(json.loads(os.environ['CI_RESULTS']))
